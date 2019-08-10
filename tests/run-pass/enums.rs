enum MyEnum {
    MyEmptyVariant,
    MyNewtypeVariant(i32),
    MyTupleVariant(i32, i32),
    MyStructVariant {
        my_first_field: i32,
        my_second_field: i32,
    }
}

fn test(me: MyEnum) {
    match me {
        MyEnum::MyEmptyVariant => {},
        MyEnum::MyNewtypeVariant(ref val) => assert_eq!(val, &42),
        MyEnum::MyTupleVariant(ref a, ref b) => {
            assert_eq!(a, &43);
            assert_eq!(b, &44);
        },
        MyEnum::MyStructVariant { ref my_first_field, ref my_second_field } => {
            assert_eq!(my_first_field, &45);
            assert_eq!(my_second_field, &46);
        },
    }
}

fn discriminant_overflow() {
    // Tests for https://github.com/rust-lang/rust/issues/62138.
    #[repr(u8)]
    #[allow(dead_code)]
    enum WithWraparoundInvalidValues {
        X = 1,
        Y = 254,
    }

    #[allow(dead_code)]
    enum Foo {
        A,
        B,
        C(WithWraparoundInvalidValues),
    }

    let x = Foo::B;
    if let Foo::C(_) = x {
        panic!();
    }
}

fn main() {
    test(MyEnum::MyEmptyVariant);
    test(MyEnum::MyNewtypeVariant(42));
    test(MyEnum::MyTupleVariant(43, 44));
    test(MyEnum::MyStructVariant{
        my_first_field: 45,
        my_second_field: 46,
    });

    discriminant_overflow();
}
