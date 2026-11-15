//! Build a small MBC1 module by hand, round-trip it through the binary format, then run it.
use geode::{
    decode, encode, verify, Func, Heap, Host, Limits, LoadError, Module, NoHost, Op, Trap, Value,
    Vm,
};

/// A deterministic host that provides one import, `twice`.
struct Doubler;

impl Host for Doubler {
    fn call(&mut self, name: &str, args: &[Value], _heap: &mut Heap) -> Result<Value, Trap> {
        match (name, args) {
            ("twice", [Value::Int(x)]) => x.checked_mul(2).map(Value::Int).ok_or(Trap::Overflow),
            _ => Err(Trap::Host(format!("unknown host call {name}"))),
        }
    }
}

fn main() {
    // main() = twice(mul(6, 7)); mul(a, b) = a * b; div() = 1 / 0.
    let module = Module {
        consts: Vec::new(),
        imports: vec!["twice".to_string()],
        funcs: vec![
            Func {
                name: "main".to_string(),
                params: 0,
                regs: 4,
                code: vec![
                    Op::Int(0, 6),
                    Op::Int(1, 7),
                    Op::Call(2, 1, 0, 2),
                    Op::CallHost(3, 0, 2, 1),
                    Op::Ret(3),
                ],
            },
            Func {
                name: "mul".to_string(),
                params: 2,
                regs: 3,
                code: vec![Op::Mul(2, 0, 1), Op::Ret(2)],
            },
            Func {
                name: "div".to_string(),
                params: 0,
                regs: 3,
                code: vec![Op::Int(0, 1), Op::Int(1, 0), Op::Div(2, 0, 1), Op::Ret(2)],
            },
        ],
        entry: 0,
    };

    println!("1. verify: {:?}", verify(&module));

    let bytes = encode(&module);
    println!(
        "2. encoded {} bytes, starts with {:?}",
        bytes.len(),
        &bytes[..4]
    );
    match decode(&bytes) {
        Ok(back) => println!("3. decoded, equal to original: {}", back == module),
        Err(e) => println!("3. decode failed: {e:?}"),
    }

    match Vm::new(module.clone(), &[]) {
        Err(LoadError::MissingImport(name)) => {
            println!("4. load without host: missing import '{name}'")
        }
        other => println!("4. load without host: unexpected {other:?}"),
    }

    let mut vm = match Vm::new(module.clone(), &["twice"]) {
        Ok(vm) => vm,
        Err(e) => {
            println!("load failed: {e:?}");
            return;
        }
    };
    match vm.run(&mut Doubler, Limits::default()) {
        Ok(v) => println!("5. run main: {v:?}, fuel used {}", vm.fuel_used()),
        Err(t) => println!("5. run main trapped: {t:?}"),
    }

    match vm.call(
        "mul",
        &[Value::Int(9), Value::Int(9)],
        &mut NoHost,
        Limits::default(),
    ) {
        Ok(v) => println!("6. call mul(9, 9): {v:?}"),
        Err(t) => println!("6. call mul(9, 9) trapped: {t:?}"),
    }

    let tight = Limits {
        fuel: 2,
        ..Limits::default()
    };
    match vm.run(&mut Doubler, tight) {
        Ok(v) => println!("7. run with fuel 2: {v:?}"),
        Err(t) => println!("7. run with fuel 2 trapped: {t:?}"),
    }

    if let Ok(mut vm) = Vm::new(module, &["twice"]) {
        let r = vm.call("div", &[], &mut NoHost, Limits::default());
        println!("8. call div: {r:?}, trap site {:?}", vm.trap_site());
    }
}
