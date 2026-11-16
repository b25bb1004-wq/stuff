//! Execution of compiled Elysium modules with the Logos standard host and property generator.
use crate::meta::{decode_meta, FuncSig};
use crate::Compiled;
use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use elysium::{Heap, Host, Limits, Obj, Value, Vm};
use logos::eval::{Outcome, Trap};
use logos::ir::{Ty, TypeInfo, TypeKind};
use logos::Span;

const MAX_CALL_DEPTH: u32 = 10_000;
const MAX_HEAP_BYTES: usize = usize::MAX / 2;

/// SplitMix64 deterministic pseudo-random generator matching `logos::eval`.
#[derive(Clone, Debug)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.u64() % (hi - lo)
    }

    fn below(&mut self, n: usize) -> usize {
        self.range(0, n as u64) as usize
    }

    fn chance(&mut self, num: u64, den: u64) -> bool {
        self.range(0, den) < num
    }
}

struct MicaHost<'a> {
    input: &'a str,
    input_pos: usize,
    output: String,
    rand_rng: Rng,
}

impl<'a> MicaHost<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            input_pos: 0,
            output: String::new(),
            rand_rng: Rng::new(1),
        }
    }
}

impl Host for MicaHost<'_> {
    fn call(
        &mut self,
        name: &str,
        args: &[Value],
        heap: &mut Heap,
    ) -> Result<Value, elysium::Trap> {
        match name {
            "io.print" => {
                if let Some(Value::Ref(h)) = args.first().copied() {
                    if let Ok(Obj::Str(s)) = heap.get(h) {
                        self.output.push_str(s);
                    }
                }
                Ok(Value::Unit)
            }
            "io.read_line" => {
                if self.input_pos >= self.input.len() {
                    let h = heap.alloc(Obj::Variant {
                        tag: 0,
                        fields: Vec::new(),
                    })?;
                    return Ok(Value::Ref(h));
                }
                let rest = &self.input[self.input_pos..];
                let line = match rest.find('\n') {
                    Some(nl) => {
                        self.input_pos += nl + 1;
                        let s = &rest[..nl];
                        s.strip_suffix('\r').unwrap_or(s)
                    }
                    None => {
                        self.input_pos = self.input.len();
                        rest
                    }
                };
                let str_h = heap.alloc(Obj::Str(String::from(line)))?;
                let opt_h = heap.alloc(Obj::Variant {
                    tag: 1,
                    fields: vec![Value::Ref(str_h)],
                })?;
                Ok(Value::Ref(opt_h))
            }
            "time.now" => Ok(Value::Int(0)),
            "rand.next" => Ok(Value::Int(self.rand_rng.u64() as i64)),
            other => Err(elysium::Trap::Host(String::from(other))),
        }
    }
}

/// Runs `main` of `c` on Elysium with `input` for `io.read_line` and a `fuel` budget.
pub fn run_main(c: &Compiled, input: &str, fuel: u64) -> Outcome {
    run_main_value(c, input, fuel).0
}

/// `run_main`, also rendering the value `main` returns as `elysium --json run` renders it (None for unit).
pub fn run_main_value(c: &Compiled, input: &str, fuel: u64) -> (Outcome, Option<String>) {
    let Some(fid) = c.main else {
        return (
            Outcome {
                output: String::new(),
                result: Ok(()),
            },
            None,
        );
    };
    let Some(mut vm) = make_vm(c) else {
        return (err_outcome(Trap::IndexOutOfRange), None);
    };
    let mut host = MicaHost::new(input);
    let (result, value) = match exec_func(c, &mut vm, fid, &[], &mut host, fuel) {
        Ok(Value::Unit) => (Ok(()), None),
        Ok(val) => {
            let mut seen = BTreeSet::new();
            (Ok(()), Some(fmt_value(val, vm.heap(), &mut seen, 0)))
        }
        Err(e) => (Err(e), None),
    };
    (
        Outcome {
            output: host.output,
            result,
        },
        value,
    )
}

/// Renders a value like `fmt_value` in elysium's `src/bin/elysium.rs`; keep the two in step.
fn fmt_value(val: Value, heap: &Heap, seen: &mut BTreeSet<u32>, depth: usize) -> String {
    match val {
        Value::Unit => String::from("()"),
        Value::Int(i) => i.to_string(),
        Value::Byte(b) => format!("{b}u8"),
        Value::Bool(b) => b.to_string(),
        Value::Fn(f) => format!("fn#{f}"),
        Value::Ref(h) => {
            if depth >= 32 {
                return format!("ref#{}(...)", h.0);
            }
            if !seen.insert(h.0) {
                return format!("ref#{}(<cycle>)", h.0);
            }
            let rendered = match heap.get(h) {
                Ok(Obj::Str(s)) => format!("{s:?}"),
                Ok(Obj::Bytes(b)) => {
                    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
                    format!("bytes[{}]({hex})", b.len())
                }
                Ok(Obj::Array(items)) => format!("[{}]", join_values(items, heap, seen, depth)),
                Ok(Obj::Record(fields)) => {
                    format!("{{{}}}", join_values(fields, heap, seen, depth))
                }
                Ok(Obj::Variant { tag, fields }) => {
                    format!("variant#{tag}({})", join_values(fields, heap, seen, depth))
                }
                Err(_) => format!("ref#{}(<dangling>)", h.0),
            };
            seen.remove(&h.0);
            rendered
        }
    }
}

fn join_values(items: &[Value], heap: &Heap, seen: &mut BTreeSet<u32>, depth: usize) -> String {
    let parts: Vec<String> = items
        .iter()
        .map(|v| fmt_value(*v, heap, seen, depth + 1))
        .collect();
    parts.join(", ")
}

/// Runs unit test `index` of `c` on Elysium with a `fuel` budget.
pub fn run_test(c: &Compiled, index: usize, fuel: u64) -> Outcome {
    let Some((_, fid)) = c.tests.get(index) else {
        return err_outcome(Trap::IndexOutOfRange);
    };
    let Some(mut vm) = make_vm(c) else {
        return err_outcome(Trap::IndexOutOfRange);
    };
    let mut host = MicaHost::new("");
    let result = exec_func(c, &mut vm, *fid, &[], &mut host, fuel).map(|_| ());
    Outcome {
        output: host.output,
        result,
    }
}

/// Runs property test `index` of `c` on Elysium for up to `cases` seeded inputs with `fuel` per case.
pub fn run_prop(c: &Compiled, index: usize, seed: u64, cases: u32, fuel: u64) -> Outcome {
    let Some(prop) = c.props.get(index) else {
        return err_outcome(Trap::IndexOutOfRange);
    };
    if c.module.funcs.get(prop.body as usize).is_none() {
        return err_outcome(Trap::IndexOutOfRange);
    }
    let Some(base_vm) = make_vm(c) else {
        return err_outcome(Trap::IndexOutOfRange);
    };
    let (types, func_sigs) = decode_meta(&c.module.consts);
    let mut master = Rng::new(seed);
    let mut output = String::new();

    for case in 0..cases {
        let case_seed = master.u64();
        let mut rng = Rng::new(case_seed);
        let gen_args: Vec<GenVal> = prop
            .params
            .iter()
            .map(|ty| gen_val(&types, &func_sigs, ty, &mut rng, 0))
            .collect();

        if let Some(guard_fid) = prop.guard {
            let mut gvm = base_vm.clone();
            let guard_args = materialize_all(&gen_args, gvm.heap_mut());
            let mut ghost = MicaHost::new("");
            match exec_func(c, &mut gvm, guard_fid, &guard_args, &mut ghost, fuel) {
                Ok(Value::Bool(true)) => {}
                Ok(Value::Bool(false)) => continue,
                Ok(_) => {
                    return Outcome {
                        output,
                        result: Err((Trap::IndexOutOfRange, Span { start: 0, end: 0 })),
                    };
                }
                Err((trap, span)) => {
                    output.push_str(&format!(
                        "prop '{}' guard failed on case {} (seed {:#018x}): {:?}\n",
                        prop.name, case, seed, trap
                    ));
                    return Outcome {
                        output,
                        result: Err((trap, span)),
                    };
                }
            }
        }

        let mut vm = base_vm.clone();
        let body_args = materialize_all(&gen_args, vm.heap_mut());
        let mut host = MicaHost::new("");
        if let Err((trap, span)) = exec_func(c, &mut vm, prop.body, &body_args, &mut host, fuel) {
            output.push_str(&host.output);
            output.push_str(&format!(
                "prop '{}' failed on case {} (seed {:#018x}, case_seed {:#018x}): {:?}\n",
                prop.name, case, seed, case_seed, trap
            ));
            return Outcome {
                output,
                result: Err((trap, span)),
            };
        }
    }

    Outcome {
        output,
        result: Ok(()),
    }
}

fn make_vm(c: &Compiled) -> Option<Vm> {
    let host_names: Vec<&str> = c.module.imports.iter().map(String::as_str).collect();
    Vm::new(c.module.clone(), &host_names).ok()
}

fn exec_func(
    c: &Compiled,
    vm: &mut Vm,
    fid: u32,
    args: &[Value],
    host: &mut MicaHost<'_>,
    fuel: u64,
) -> Result<Value, (Trap, Span)> {
    let Some(f_def) = c.module.funcs.get(fid as usize) else {
        return Err((Trap::IndexOutOfRange, Span { start: 0, end: 0 }));
    };
    let name = f_def.name.clone();
    let limits = Limits {
        fuel,
        max_heap_bytes: MAX_HEAP_BYTES,
        max_depth: MAX_CALL_DEPTH,
    };
    vm.call(&name, args, host, limits).map_err(|gtrap| {
        let span = vm
            .trap_site()
            .and_then(|(f_idx, op_idx)| {
                c.spans
                    .get(f_idx as usize)
                    .and_then(|s| s.get(op_idx as usize))
                    .copied()
            })
            .unwrap_or(Span { start: 0, end: 0 });
        (map_trap(gtrap), span)
    })
}

fn map_trap(t: elysium::Trap) -> Trap {
    match t {
        elysium::Trap::Overflow => Trap::Overflow,
        elysium::Trap::DivideByZero => Trap::DivideByZero,
        elysium::Trap::IndexOutOfRange => Trap::IndexOutOfRange,
        elysium::Trap::AssertFailed(msg) => Trap::AssertFailed(msg),
        elysium::Trap::OutOfFuel => Trap::OutOfFuel,
        elysium::Trap::StackOverflow => Trap::StackOverflow,
        elysium::Trap::Host(name) => Trap::NoExtern(name),
        elysium::Trap::TypeError | elysium::Trap::OutOfMemory | elysium::Trap::User(_) => {
            Trap::IndexOutOfRange
        }
    }
}

fn err_outcome(trap: Trap) -> Outcome {
    Outcome {
        output: String::new(),
        result: Err((trap, Span { start: 0, end: 0 })),
    }
}

#[derive(Clone)]
enum GenVal {
    Unit,
    Int(i64),
    Byte(u8),
    Bool(bool),
    Fn(u32),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<GenVal>),
    Record(Vec<GenVal>),
    Variant(u32, Vec<GenVal>),
}

fn materialize_all(vals: &[GenVal], heap: &mut Heap) -> Vec<Value> {
    vals.iter().map(|v| materialize(v, heap)).collect()
}

fn materialize(v: &GenVal, heap: &mut Heap) -> Value {
    let obj = match v {
        GenVal::Unit => return Value::Unit,
        GenVal::Int(n) => return Value::Int(*n),
        GenVal::Byte(b) => return Value::Byte(*b),
        GenVal::Bool(b) => return Value::Bool(*b),
        GenVal::Fn(f) => return Value::Fn(*f),
        GenVal::Str(s) => Obj::Str(s.clone()),
        GenVal::Bytes(b) => Obj::Bytes(b.clone()),
        GenVal::Array(xs) => Obj::Array(materialize_all(xs, heap)),
        GenVal::Record(xs) => Obj::Record(materialize_all(xs, heap)),
        GenVal::Variant(tag, xs) => Obj::Variant {
            tag: *tag,
            fields: materialize_all(xs, heap),
        },
    };
    heap.alloc(obj).map_or(Value::Unit, Value::Ref)
}

fn gen_val(
    types: &[TypeInfo],
    func_sigs: &[FuncSig],
    ty: &Ty,
    rng: &mut Rng,
    depth: u32,
) -> GenVal {
    match ty {
        Ty::Int => GenVal::Int(if rng.chance(1, 2) {
            (rng.range(0, 2001) as i64) - 1000
        } else {
            rng.u64() as i64
        }),
        Ty::Byte => GenVal::Byte(rng.u64() as u8),
        Ty::Bool => GenVal::Bool(rng.chance(1, 2)),
        Ty::Unit => GenVal::Unit,
        Ty::Str => {
            let len = if depth > 4 { 0 } else { rng.below(17) };
            let mut s = String::with_capacity(len);
            for _ in 0..len {
                let ch = (b' ' + (rng.below(95) as u8)) as char;
                s.push(ch);
            }
            GenVal::Str(s)
        }
        Ty::Array(elem) => {
            let len = if depth > 3 { 0 } else { rng.below(13) };
            if **elem == Ty::Byte {
                let bytes = (0..len).map(|_| rng.u64() as u8).collect();
                GenVal::Bytes(bytes)
            } else {
                let items = (0..len)
                    .map(|_| gen_val(types, func_sigs, elem, rng, depth + 1))
                    .collect();
                GenVal::Array(items)
            }
        }
        Ty::Tuple(elems) => {
            let items = elems
                .iter()
                .map(|t| gen_val(types, func_sigs, t, rng, depth + 1))
                .collect();
            GenVal::Record(items)
        }
        Ty::Opt(inner) => {
            if depth > 4 || rng.chance(1, 4) {
                GenVal::Variant(0, Vec::new())
            } else {
                let v = gen_val(types, func_sigs, inner, rng, depth + 1);
                GenVal::Variant(1, vec![v])
            }
        }
        Ty::Named(tid) => match types.get(*tid as usize).map(|t| &t.kind) {
            Some(TypeKind::Record(fields)) => {
                let vals = fields
                    .iter()
                    .map(|(_, ft)| gen_val(types, func_sigs, ft, rng, depth + 1))
                    .collect();
                GenVal::Record(vals)
            }
            Some(TypeKind::Variants(vars)) if !vars.is_empty() => {
                let idx = if depth > 4 {
                    vars.iter().position(|(_, f)| f.is_empty()).unwrap_or(0)
                } else {
                    rng.below(vars.len())
                };
                let vals = vars
                    .get(idx)
                    .map(|(_, f)| {
                        f.iter()
                            .map(|ft| gen_val(types, func_sigs, ft, rng, depth + 1))
                            .collect()
                    })
                    .unwrap_or_default();
                GenVal::Variant(idx as u32, vals)
            }
            _ => GenVal::Unit,
        },
        Ty::Fn {
            params,
            ret,
            layers,
        } => {
            let matched = func_sigs.iter().enumerate().find(|(_, (fp, fr, fl))| {
                fp.as_slice() == params.as_slice() && fr == &**ret && (*fl & !*layers) == 0
            });
            let _ = Box::new(0u8);
            GenVal::Fn(matched.map_or(0, |(i, _)| i as u32))
        }
    }
}
