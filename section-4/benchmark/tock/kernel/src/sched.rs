//! Tock core scheduler.

use core::cell::Cell;
use core::ptr::NonNull;

use crate::callback::Callback;
use crate::capabilities;
use crate::common::cells::NumericCellExt;
use crate::grant::Grant;
use crate::ipc;
use crate::memop;
use crate::platform::mpu::MPU;
use crate::platform::systick::SysTick;
use crate::platform::{Chip, Platform};
use crate::process::{self, Task};
use crate::returncode::ReturnCode;
use crate::syscall::{ContextSwitchReason, Syscall};

/// The time a process is permitted to run before being pre-empted
const KERNEL_TICK_DURATION_US: u32 = 10000;
/// Skip re-scheduling a process if its quanta is nearly exhausted
const MIN_QUANTA_THRESHOLD_US: u32 = 500;

/// Main object for the kernel. Each board will need to create one.
pub struct Kernel {
    /// How many "to-do" items exist at any given time. These include
    /// outstanding callbacks and processes in the Running state.
    work: Cell<usize>,
    /// This holds a pointer to the static array of Process pointers.
    processes: &'static [Option<&'static process::ProcessType>],
    /// How many grant regions have been setup. This is incremented on every
    /// call to `create_grant()`. We need to explicitly track this so that when
    /// processes are created they can allocated pointers for each grant.
    grant_counter: Cell<usize>,
    /// Flag to mark that grants have been finalized. This means that the kernel
    /// cannot support creating new grants because processes have already been
    /// created and the data structures for grants have already been
    /// established.
    grants_finalized: Cell<bool>,
}

impl Kernel {
    pub fn new(processes: &'static [Option<&'static process::ProcessType>]) -> Kernel {
        Kernel {
            work: Cell::new(0),
            processes: processes,
            grant_counter: Cell::new(0),
            grants_finalized: Cell::new(false),
        }
    }

    /// Something was scheduled for a process, so there is more work to do.
    crate fn increment_work(&self) {
        self.work.increment();
    }

    /// Something finished for a process, so we decrement how much work there is
    /// to do.
    crate fn decrement_work(&self) {
        self.work.decrement();
    }

    /// Helper function for determining if we should service processes or go to
    /// sleep.
    fn processes_blocked(&self) -> bool {
        self.work.get() == 0
    }

    /// Run a closure on a specific process if it exists. If the process does
    /// not exist (i.e. it is `None` in the `processes` array) then `default`
    /// will be returned. Otherwise the closure will executed and passed a
    /// reference to the process.
    crate fn process_map_or<F, R>(&self, default: R, process_index: usize, closure: F) -> R
    where
        F: FnOnce(&process::ProcessType) -> R,
    {
        if process_index > self.processes.len() {
            return default;
        }
        self.processes[process_index].map_or(default, |process| closure(process))
    }

    /// Run a closure on every valid process. This will iterate the array of
    /// processes and call the closure on every process that exists.
    crate fn process_each<F>(&self, closure: F)
    where
        F: Fn(&process::ProcessType),
    {
        for process in self.processes.iter() {
            match process {
                Some(p) => {
                    closure(*p);
                }
                None => {}
            }
        }
    }

    /// Run a closure on every valid process. This will iterate the
    /// array of processes and call the closure on every process that
    /// exists. Ths method is available outside the kernel crate but
    /// requires a `ProcessManagementCapability` to use.
    pub fn process_each_capability<F>(
        &'static self,
        _capability: &capabilities::ProcessManagementCapability,
        closure: F,
    ) where
        F: Fn(usize, &process::ProcessType),
    {
        for (i, process) in self.processes.iter().enumerate() {
            match process {
                Some(p) => {
                    closure(i, *p);
                }
                None => {}
            }
        }
    }

    /// Run a closure on every process, but only continue if the closure returns
    /// `FAIL`. That is, if the closure returns any other return code than
    /// `FAIL`, that value will be returned from this function and the iteration
    /// of the array of processes will stop.
    crate fn process_until<F>(&self, closure: F) -> ReturnCode
    where
        F: Fn(&process::ProcessType) -> ReturnCode,
    {
        for process in self.processes.iter() {
            match process {
                Some(p) => {
                    let ret = closure(*p);
                    if ret != ReturnCode::FAIL {
                        return ret;
                    }
                }
                None => {}
            }
        }
        ReturnCode::FAIL
    }

    /// Return how many processes this board supports.
    crate fn number_of_process_slots(&self) -> usize {
        self.processes.len()
    }

    /// Create a new grant. This is used in board initialization to setup grants
    /// that capsules use to interact with processes.
    ///
    /// Grants **must** only be created _before_ processes are initialized.
    /// Processes use the number of grants that have been allocated to correctly
    /// initialize the process's memory with a pointer for each grant. If a
    /// grant is created after processes are initialized this will panic.
    ///
    /// Calling this function is restricted to only certain users, and to
    /// enforce this calling this function requires the
    /// `MemoryAllocationCapability` capability.
    pub fn create_grant<T: Default>(
        &'static self,
        _capability: &capabilities::MemoryAllocationCapability,
    ) -> Grant<T> {
        if self.grants_finalized.get() {
            panic!("Grants finalized. Cannot create a new grant.");
        }

        // Create and return a new grant.
        let grant_index = self.grant_counter.get();
        self.grant_counter.increment();
        Grant::new(self, grant_index)
    }

    /// Returns the number of grants that have been setup in the system and
    /// marks the grants as "finalized". This means that no more grants can
    /// be created because data structures have been setup based on the number
    /// of grants when this function is called.
    ///
    /// In practice, this is called when processes are created, and the process
    /// memory is setup based on the number of current grants.
    crate fn get_grant_count_and_finalize(&self) -> usize {
        self.grants_finalized.set(true);
        self.grant_counter.get()
    }

    /// Cause all apps to fault.
    ///
    /// This will call `set_fault_state()` on each app, causing the app to enter
    /// the state as if it had crashed (for example with an MPU violation). If
    /// the process is configured to be restarted it will be.
    ///
    /// Only callers with the `ProcessManagementCapability` can call this
    /// function. This restricts general capsules from being able to call this
    /// function, since capsules should not be able to arbitrarily restart all
    /// apps.
    pub fn hardfault_all_apps<C: capabilities::ProcessManagementCapability>(&self, _c: &C) {
        for p in self.processes.iter() {
            p.map(|process| {
                process.set_fault_state();
            });
        }
    }

    /// Main loop.
    pub fn kernel_loop<P: Platform, C: Chip>(
        &'static self,
        platform: &P,
        chip: &C,
        ipc: Option<&ipc::IPC>,
        _capability: &capabilities::MainLoopCapability,
    ) {
        loop {
            unsafe {
                chip.service_pending_interrupts();

                for p in self.processes.iter() {
                    p.map(|process| {
                        self.do_process(platform, chip, process, ipc);
                    });
                    if chip.has_pending_interrupts() {
                        break;
                    }
                }

                chip.atomic(|| {
                    if !chip.has_pending_interrupts() && self.processes_blocked() {
                        chip.sleep();
                    }
                });
            };
        }
    }

    unsafe fn do_process<P: Platform, C: Chip>(
        &self,
        platform: &P,
        chip: &C,
        process: &process::ProcessType,
        ipc: Option<&crate::ipc::IPC>,
    ) {
        let appid = process.appid();
        let systick = chip.systick();
        systick.reset();
        systick.set_timer(KERNEL_TICK_DURATION_US);
        systick.enable(false);

        loop {
            if chip.has_pending_interrupts() {
                break;
            }

            if systick.overflowed() || !systick.greater_than(MIN_QUANTA_THRESHOLD_US) {
                process.debug_timeslice_expired();
                break;
            }

            match process.get_state() {
                process::State::Running => {
                    // Running means that this process expects to be running,
                    // so go ahead and set things up and switch to executing
                    // the process.
                    process.setup_mpu();
                    chip.mpu().enable_mpu();
                    systick.enable(true);
                    let context_switch_reason = process.switch_to();
                    systick.enable(false);
                    chip.mpu().disable_mpu();

                    // Now the process has returned back to the kernel. Check
                    // why and handle the process as appropriate.
                    match context_switch_reason {
                        Some(ContextSwitchReason::Fault) => {
                            // Let process deal with it as appropriate.
                            process.set_fault_state();
                        }
                        Some(ContextSwitchReason::SyscallFired) => {
                            // Handle each of the syscalls.
                            match process.get_syscall() {
                                Some(Syscall::MEMOP { operand, arg0 }) => {
                                    let res = memop::memop(process, operand, arg0);
                                    process.set_syscall_return_value(res.into());
                                }
                                Some(Syscall::YIELD) => {
                                    process.set_yielded_state();
                                    process.pop_syscall_stack_frame();

                                    // There might be already enqueued callbacks
                                    continue;
                                }
                                Some(Syscall::SUBSCRIBE {
                                    driver_number,
                                    subdriver_number,
                                    callback_ptr,
                                    appdata,
                                }) => {
                                    let callback_ptr = NonNull::new(callback_ptr);
                                    let callback = callback_ptr
                                        .map(|ptr| Callback::new(appid, appdata, ptr.cast()));

                                    let res =
                                        platform.with_driver(
                                            driver_number,
                                            |driver| match driver {
                                                Some(d) => {
                                                    d.subscribe(subdriver_number, callback, appid)
                                                }
                                                None => ReturnCode::ENODEVICE,
                                            },
                                        );
                                    process.set_syscall_return_value(res.into());
                                }
                                Some(Syscall::COMMAND {
                                    driver_number,
                                    subdriver_number,
                                    arg0,
                                    arg1,
                                }) => {
                                    let res =
                                        platform.with_driver(
                                            driver_number,
                                            |driver| match driver {
                                                Some(d) => {
                                                    d.command(subdriver_number, arg0, arg1, appid)
                                                }
                                                None => ReturnCode::ENODEVICE,
                                            },
                                        );
                                    process.set_syscall_return_value(res.into());
                                }
                                Some(Syscall::ALLOW {
                                    driver_number,
                                    subdriver_number,
                                    allow_address,
                                    allow_size,
                                }) => {
                                    let res = platform.with_driver(driver_number, |driver| {
                                        match driver {
                                            Some(d) => {
                                                match process.allow(allow_address, allow_size) {
                                                    Ok(oslice) => {
                                                        d.allow(appid, subdriver_number, oslice)
                                                    }
                                                    Err(err) => err, /* memory not valid */
                                                }
                                            }
                                            None => ReturnCode::ENODEVICE,
                                        }
                                    });
                                    process.set_syscall_return_value(res.into());
                                }
                                _ => {}
                            }
                        }
                        Some(ContextSwitchReason::TimesliceExpired) => {
                            // break to handle other processes.
                            break;
                        }
                        Some(ContextSwitchReason::Interrupted) => {
                            // break to handle other processes.
                            break;
                        }
                        None => {
                            // Something went wrong when switching to this
                            // process. Indicate this by putting it in a fault
                            // state.
                            process.set_fault_state();
                        }
                    }
                }
                process::State::Yielded => match process.dequeue_task() {
                    // If the process is yielded it might be waiting for a
                    // callback. If there is a task scheduled for this process
                    // go ahead and set the process to execute it.
                    None => break,
                    Some(cb) => match cb {
                        Task::FunctionCall(ccb) => {
                            process.push_function_call(ccb);
                        }
                        Task::IPC((otherapp, ipc_type)) => {
                            ipc.map_or_else(
                                || {
                                    assert!(
                                        false,
                                        "Kernel consistency error: IPC Task with no IPC"
                                    );
                                },
                                |ipc| {
                                    ipc.schedule_callback(appid, otherapp, ipc_type);
                                },
                            );
                        }
                    },
                },
                process::State::Fault => {
                    // We should never be scheduling a process in fault.
                    panic!("Attempted to schedule a faulty process");
                }
                process::State::StoppedRunning => {
                    break;
                    // Do nothing
                }
                process::State::StoppedYielded => {
                    break;
                    // Do nothing
                }
                process::State::StoppedFaulted => {
                    break;
                    // Do nothing
                }
            }
        }
        systick.reset();
    }
}
