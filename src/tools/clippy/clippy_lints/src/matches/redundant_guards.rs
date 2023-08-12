use clippy_utils::diagnostics::span_lint_and_then;
use clippy_utils::path_to_local;
use clippy_utils::source::snippet_with_applicability;
use clippy_utils::visitors::{for_each_expr, is_local_used};
use rustc_ast::LitKind;
use rustc_errors::Applicability;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::{Arm, BinOpKind, Expr, ExprKind, Guard, MatchSource, Node, Pat, PatKind};
use rustc_lint::LateContext;
use rustc_span::Span;
use std::ops::ControlFlow;

use super::REDUNDANT_GUARDS;

pub(super) fn check<'tcx>(cx: &LateContext<'tcx>, arms: &'tcx [Arm<'tcx>]) {
    for outer_arm in arms {
        let Some(guard) = outer_arm.guard else {
            continue;
        };

        // `Some(x) if matches!(x, y)`
        if let Guard::If(if_expr) = guard
            && let ExprKind::Match(
                scrutinee,
                [
                    arm,
                    Arm {
                        pat: Pat {
                            kind: PatKind::Wild,
                            ..
                        },
                        ..
                    },
                ],
                MatchSource::Normal,
            ) = if_expr.kind
        {
            emit_redundant_guards(
                cx,
                outer_arm,
                if_expr.span,
                scrutinee,
                arm.pat.span,
                arm.guard,
            );
        }
        // `Some(x) if let Some(2) = x`
        else if let Guard::IfLet(let_expr) = guard {
            emit_redundant_guards(
                cx,
                outer_arm,
                let_expr.span,
                let_expr.init,
                let_expr.pat.span,
                None,
            );
        }
        // `Some(x) if x == Some(2)`
        else if let Guard::If(if_expr) = guard
            && let ExprKind::Binary(bin_op, local, pat) = if_expr.kind
            && matches!(bin_op.node, BinOpKind::Eq)
            && expr_can_be_pat(cx, pat)
            // Ensure they have the same type. If they don't, we'd need deref coercion which isn't
            // possible (currently) in a pattern. In some cases, you can use something like
            // `as_deref` or similar but in general, we shouldn't lint this as it'd create an
            // extraordinary amount of FPs.
            //
            // This isn't necessary in the other two checks, as they must be a pattern already.
            && cx.typeck_results().expr_ty(local) == cx.typeck_results().expr_ty(pat)
        {
            emit_redundant_guards(
                cx,
                outer_arm,
                if_expr.span,
                local,
                pat.span,
                None,
            );
        }
    }
}

fn get_pat_binding<'tcx>(cx: &LateContext<'tcx>, guard_expr: &Expr<'_>, outer_arm: &Arm<'tcx>) -> Option<(Span, bool)> {
    if let Some(local) = path_to_local(guard_expr) && !is_local_used(cx, outer_arm.body, local) {
        let mut span = None;
        let mut multiple_bindings = false;
        // `each_binding` gives the `HirId` of the `Pat` itself, not the binding
        outer_arm.pat.walk(|pat| {
            if let PatKind::Binding(_, hir_id, _, _) = pat.kind
                && hir_id == local
                && span.replace(pat.span).is_some()
            {
                multiple_bindings = true;
                return false;
            }

            true
        });

        // Ignore bindings from or patterns, like `First(x) | Second(x, _) | Third(x, _, _)`
        if !multiple_bindings {
            return span.map(|span| {
                (
                    span,
                    !matches!(cx.tcx.hir().get_parent(local), Node::PatField(_)),
                )
            });
        }
    }

    None
}

fn emit_redundant_guards<'tcx>(
    cx: &LateContext<'tcx>,
    outer_arm: &Arm<'tcx>,
    guard_span: Span,
    local: &Expr<'_>,
    pat_span: Span,
    inner_guard: Option<Guard<'_>>,
) {
    let mut app = Applicability::MaybeIncorrect;
    let Some((pat_binding, can_use_shorthand)) = get_pat_binding(cx, local, outer_arm) else {
        return;
    };

    span_lint_and_then(
        cx,
        REDUNDANT_GUARDS,
        guard_span.source_callsite(),
        "redundant guard",
        |diag| {
            let binding_replacement = snippet_with_applicability(cx, pat_span, "<binding_repl>", &mut app);
            diag.multipart_suggestion_verbose(
                "try",
                vec![
                    if can_use_shorthand {
                        (pat_binding, binding_replacement.into_owned())
                    } else {
                        (pat_binding.shrink_to_hi(), format!(": {binding_replacement}"))
                    },
                    (
                        guard_span.source_callsite().with_lo(outer_arm.pat.span.hi()),
                        inner_guard.map_or_else(String::new, |guard| {
                            let (prefix, span) = match guard {
                                Guard::If(e) => ("if", e.span),
                                Guard::IfLet(l) => ("if let", l.span),
                            };

                            format!(
                                " {prefix} {}",
                                snippet_with_applicability(cx, span, "<guard>", &mut app),
                            )
                        }),
                    ),
                ],
                app,
            );
        },
    );
}

/// Checks if the given `Expr` can also be represented as a `Pat`.
///
/// All literals generally also work as patterns, however float literals are special.
/// They are currently (as of 2023/08/08) still allowed in patterns, but that will become
/// an error in the future, and rustc already actively warns against this (see rust#41620),
/// so we don't consider those as usable within patterns for linting purposes.
fn expr_can_be_pat(cx: &LateContext<'_>, expr: &Expr<'_>) -> bool {
    for_each_expr(expr, |expr| {
        if match expr.kind {
            ExprKind::ConstBlock(..) => cx.tcx.features().inline_const_pat,
            ExprKind::Call(c, ..) if let ExprKind::Path(qpath) = c.kind => {
                // Allow ctors
                matches!(cx.qpath_res(&qpath, c.hir_id), Res::Def(DefKind::Ctor(..), ..))
            },
            ExprKind::Path(qpath) => {
                matches!(
                    cx.qpath_res(&qpath, expr.hir_id),
                    Res::Def(DefKind::Struct | DefKind::Enum | DefKind::Ctor(..), ..),
                )
            },
            ExprKind::AddrOf(..)
            | ExprKind::Array(..)
            | ExprKind::Tup(..)
            | ExprKind::Struct(..) => true,
            ExprKind::Lit(lit) if !matches!(lit.node, LitKind::Float(..)) => true,
            _ => false,
        } {
            return ControlFlow::Continue(());
        }

        ControlFlow::Break(())
    })
    .is_none()
}
