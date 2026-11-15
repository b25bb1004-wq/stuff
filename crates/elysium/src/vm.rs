//! The VM entry point, host interface, resource limits and module loader.

use crate::exec;
use crate::heap::{Heap, DEFAULT_MAX_HEAP_BYTES};
use crate::module::Module;
use crate::value::{Trap, Value};
use crate::verify::{verify, VerifyError};
use alloc::string::String;

/// Interface implemented by the embedder to provide imported host functions.
pub trait Host {
    /// Invoke the named host function with `args` and access to the VM `heap`.
    fn call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Result<Value, Trap>;
}

/// A host that provides no functions and traps on every call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoHost;

impl Host for NoHost {
    fn call(&mut self, _name: &str, _args: &[Value], _heap: &mut Heap) -> Result<Value, Trap> {
        Err(Trap::Host(String::from("no host")))
    }
}

/// Resource limits for a single VM `run` or `call`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum fuel units the run may spend.
    pub fuel: u64,
    /// Maximum live heap bytes permitted.
    pub max_heap_bytes: usize,
    /// Maximum call frame depth permitted.
    pub max_depth: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            fuel: 10_000_000,
            max_heap_bytes: DEFAULT_MAX_HEAP_BYTES,
            max_depth: 1024,
        }
    }
}

/// Why a module could not be loaded into a `Vm`.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadError {
    /// The module failed static verification.
    Verify(VerifyError),
    /// The module imports a host function name not provided by the embedder.
    MissingImport(String),
}

/// A loaded, verified bytecode virtual machine instance.
#[derive(Debug, Clone)]
pub struct Vm {
    pub(crate) module: Module,
    pub(crate) heap: Heap,
    pub(crate) fuel_used: u64,
    pub(crate) trap_site: Option<(u32, u32)>,
}

impl Vm {
    /// Verify `module` and check that every import is listed in `host_names`.
    pub fn new(module: Module, host_names: &[&str]) -> Result<Vm, LoadError> {
        verify(&module).map_err(LoadError::Verify)?;
        for imp in &module.imports {
            if !host_names.contains(&imp.as_str()) {
                return Err(LoadError::MissingImport(imp.clone()));
            }
        }
        Ok(Vm {
            module,
            heap: Heap::new(DEFAULT_MAX_HEAP_BYTES),
            fuel_used: 0,
            trap_site: None,
        })
    }

    /// Run the module's `entry` function with no arguments.
    pub fn run(&mut self, host: &mut dyn Host, limits: Limits) -> Result<Value, Trap> {
        self.fuel_used = 0;
        self.trap_site = None;
        self.heap.set_max_bytes(limits.max_heap_bytes);
        let entry = self.module.entry;
        let func = self
            .module
            .funcs
            .get(entry as usize)
            .ok_or(Trap::TypeError)?;
        if func.params != 0 {
            return Err(Trap::TypeError);
        }
        exec::execute(self, entry, &[], host, limits)
    }

    /// Call the function named `func` with `args`.
    pub fn call(
        &mut self,
        func: &str,
        args: &[Value],
        host: &mut dyn Host,
        limits: Limits,
    ) -> Result<Value, Trap> {
        self.fuel_used = 0;
        self.trap_site = None;
        self.heap.set_max_bytes(limits.max_heap_bytes);
        let (f_idx, f_def) = self
            .module
            .funcs
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == func)
            .ok_or(Trap::TypeError)?;
        if usize::from(f_def.params) != args.len() {
            return Err(Trap::TypeError);
        }
        let f_u32 = u32::try_from(f_idx).map_err(|_| Trap::TypeError)?;
        exec::execute(self, f_u32, args, host, limits)
    }

    /// Borrow the VM's object heap.
    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    /// Mutably borrow the VM's object heap.
    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }

    /// Fuel spent by completed instructions during the last `run` or `call`.
    pub fn fuel_used(&self) -> u64 {
        self.fuel_used
    }

    /// Function index and instruction index of the op that raised the last trap; `None` after a clean run.
    pub fn trap_site(&self) -> Option<(u32, u32)> {
        self.trap_site
    }
}
