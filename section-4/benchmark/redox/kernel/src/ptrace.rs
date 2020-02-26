//! The backend of the "proc:" scheme. Most internal breakpoint
//! handling should go here, unless they closely depend on the design
//! of the scheme.

use crate::{
    arch::{
        macros::InterruptStack,
        paging::{
            entry::EntryFlags,
            mapper::MapperFlushAll,
            temporary_page::TemporaryPage,
            ActivePageTable, InactivePageTable, Page, PAGE_SIZE, VirtualAddress
        }
    },
    common::unique::Unique,
    context::{self, signal, Context, ContextId, Status},
    event,
    scheme::proc,
    sync::WaitCondition,
    syscall::{
        data::PtraceEvent,
        error::*,
        flag::*,
        ptrace_event
    },
};

use alloc::{
    boxed::Box,
    collections::{
        BTreeMap,
        VecDeque,
        btree_map::Entry
    },
    sync::Arc,
    vec::Vec
};
use core::{
    cmp,
    sync::atomic::Ordering
};
use spin::{Once, RwLock, RwLockReadGuard, RwLockWriteGuard};

//  ____                _
// / ___|  ___  ___ ___(_) ___  _ __  ___
// \___ \ / _ \/ __/ __| |/ _ \| '_ \/ __|
//  ___) |  __/\__ \__ \ | (_) | | | \__ \
// |____/ \___||___/___/_|\___/|_| |_|___/

#[derive(Debug)]
struct Session {
    breakpoint: Option<Breakpoint>,
    events: VecDeque<PtraceEvent>,
    file_id: usize,
    tracee: Arc<WaitCondition>,
    tracer: Arc<WaitCondition>,
}
impl Session {
    fn send_event(&mut self, event: PtraceEvent) {
        self.events.push_back(event);

        // Notify nonblocking tracers
        if self.events.len() == 1 {
            // If the list of events was previously empty, alert now
            proc_trigger_event(self.file_id, EVENT_READ);
        }

        // Alert blocking tracers
        self.tracer.notify();
    }
}

type SessionMap = BTreeMap<ContextId, Session>;

static SESSIONS: Once<RwLock<SessionMap>> = Once::new();

fn init_sessions() -> RwLock<SessionMap> {
    RwLock::new(BTreeMap::new())
}
fn sessions() -> RwLockReadGuard<'static, SessionMap> {
    SESSIONS.call_once(init_sessions).read()
}
fn sessions_mut() -> RwLockWriteGuard<'static, SessionMap> {
    SESSIONS.call_once(init_sessions).write()
}

/// Try to create a new session, but fail if one already exists for
/// this process
pub fn try_new_session(pid: ContextId, file_id: usize) -> bool {
    let mut sessions = sessions_mut();

    match sessions.entry(pid) {
        Entry::Occupied(_) => false,
        Entry::Vacant(vacant) => {
            vacant.insert(Session {
                breakpoint: None,
                events: VecDeque::new(),
                file_id,
                tracee: Arc::new(WaitCondition::new()),
                tracer: Arc::new(WaitCondition::new()),
            });
            true
        }
    }
}

/// Returns true if a session is attached to this process
pub fn is_traced(pid: ContextId) -> bool {
    sessions().contains_key(&pid)
}

/// Used for getting the flags in fevent
pub fn session_fevent_flags(pid: ContextId) -> Option<EventFlags> {
    let sessions = sessions();
    let session = sessions.get(&pid)?;
    let mut flags = EventFlags::empty();
    if !session.events.is_empty() {
        flags |= EVENT_READ;
    }
    Some(flags)
}

/// Remove the session from the list of open sessions and notify any
/// waiting processes
pub fn close_session(pid: ContextId) {
    if let Some(session) = sessions_mut().remove(&pid) {
        session.tracer.notify();
        session.tracee.notify();
    }
}

/// Trigger a notification to the event: scheme
fn proc_trigger_event(file_id: usize, flags: EventFlags) {
    event::trigger(proc::PROC_SCHEME_ID.load(Ordering::SeqCst), file_id, flags);
}

/// Dispatch an event to any tracer tracing `self`. This will cause
/// the tracer to wake up and poll for events. Returns Some(()) if an
/// event was sent.
pub fn send_event(event: PtraceEvent) -> Option<()> {
    let contexts = context::contexts();
    let context = contexts.current()?;
    let context = context.read();

    let mut sessions = sessions_mut();
    let session = sessions.get_mut(&context.id)?;
    let breakpoint = session.breakpoint.as_ref()?;

    if event.cause & breakpoint.flags != event.cause {
        return None;
    }

    session.send_event(event);

    Some(())
}

/// Poll events, return the amount read
pub fn recv_events(pid: ContextId, out: &mut [PtraceEvent]) -> Option<usize> {
    let mut sessions = sessions_mut();
    let session = sessions.get_mut(&pid)?;

    let len = cmp::min(out.len(), session.events.len());
    for (dst, src) in out.iter_mut().zip(session.events.drain(..len)) {
        *dst = src;
    }
    Some(len)
}

//  ____                 _                _       _
// | __ ) _ __ ___  __ _| | ___ __   ___ (_)_ __ | |_ ___
// |  _ \| '__/ _ \/ _` | |/ / '_ \ / _ \| | '_ \| __/ __|
// | |_) | | |  __/ (_| |   <| |_) | (_) | | | | | |_\__ \
// |____/|_|  \___|\__,_|_|\_\ .__/ \___/|_|_| |_|\__|___/
//                           |_|

#[derive(Debug)]
struct Breakpoint {
    reached: bool,
    flags: PtraceFlags
}

/// Continue the process with the specified ID
pub fn cont(pid: ContextId) {
    let mut sessions = sessions_mut();
    let session = match sessions.get_mut(&pid) {
        Some(session) => session,
        None => return
    };

    // Remove the breakpoint to make sure any yet unreached but
    // obsolete breakpoints don't stop the program.
    session.breakpoint = None;

    session.tracee.notify();
}

/// Create a new breakpoint for the specified tracee, optionally with
/// a sysemu flag. Panics if the session is invalid.
pub fn set_breakpoint(pid: ContextId, flags: PtraceFlags) {
    let mut sessions = sessions_mut();
    let session = sessions.get_mut(&pid).expect("proc (set_breakpoint): invalid session");
    session.breakpoint = Some(Breakpoint {
        reached: false,
        flags
    });
}

/// Wait for the tracee to stop. If an event occurs, it returns a copy
/// of that. It will still be available for read using recv_event.
///
/// Note: Don't call while holding any locks or allocated data, this
/// will switch contexts and may in fact just never terminate.
pub fn wait(pid: ContextId) -> Result<()> {
    let tracer: Arc<WaitCondition> = {
        let sessions = sessions();
        match sessions.get(&pid) {
            Some(session) if session.breakpoint.as_ref().map(|b| !b.reached).unwrap_or(true) => {
                if !session.events.is_empty() {
                    return Ok(());
                }
                Arc::clone(&session.tracer)
            },
            _ => return Ok(())
        }
    };

    while !tracer.wait() {}

    let contexts = context::contexts();
    let context = contexts.get(pid).ok_or(Error::new(ESRCH))?;
    let context = context.read();
    if let Status::Exited(_) = context.status {
        return Err(Error::new(ESRCH));
    }

    Ok(())
}

/// Notify the tracer and await green flag to continue.
///
/// Note: Don't call while holding any locks or allocated data, this
/// will switch contexts and may in fact just never terminate.
pub fn breakpoint_callback(match_flags: PtraceFlags, event: Option<PtraceEvent>) -> Option<PtraceFlags> {
    // Can't hold any locks when executing wait()
    let (tracee, flags) = {
        let contexts = context::contexts();
        let context = contexts.current()?;
        let context = context.read();

        let mut sessions = sessions_mut();
        let session = sessions.get_mut(&context.id)?;
        let breakpoint = session.breakpoint.as_mut()?;

        if breakpoint.flags & match_flags != match_flags {
            return None;
        }

        // In case no tracer is waiting, make sure the next one gets
        // the memo
        breakpoint.reached = true;

        let flags = breakpoint.flags;
        session.send_event(event.unwrap_or(ptrace_event!(match_flags)));

        (
            Arc::clone(&session.tracee),
            flags
        )
    };

    while !tracee.wait() {}

    Some(flags)
}

/// Obtain the next breakpoint flags for the current process. This is
/// used for detecting whether or not the tracer decided to use sysemu
/// mode.
pub fn next_breakpoint() -> Option<PtraceFlags> {
    let contexts = context::contexts();
    let context = contexts.current()?;
    let context = context.read();

    let sessions = sessions();
    let session = sessions.get(&context.id)?;
    let breakpoint = session.breakpoint.as_ref()?;

    Some(breakpoint.flags)
}

/// Call when a context is closed to alert any tracers
pub fn close_tracee(pid: ContextId) -> Option<()> {
    let mut sessions = sessions_mut();
    let session = sessions.get_mut(&pid)?;

    // Cause tracers to wake up. Any action will cause ESRCH which can
    // be used to detect exit.
    session.breakpoint = None;
    session.tracer.notify();
    proc_trigger_event(session.file_id, EVENT_READ);

    Some(())
}

//  ____            _     _
// |  _ \ ___  __ _(_)___| |_ ___ _ __ ___
// | |_) / _ \/ _` | / __| __/ _ \ '__/ __|
// |  _ <  __/ (_| | \__ \ ||  __/ |  \__ \
// |_| \_\___|\__, |_|___/\__\___|_|  |___/
//            |___/

pub struct ProcessRegsGuard;

/// Make all registers available to e.g. the proc: scheme
/// ---
/// For use inside arch-specific code to assign the pointer of the
/// interupt stack to the current process. Meant to reduce the amount
/// of ptrace-related code that has to lie in arch-specific bits.
/// ```rust,ignore
/// let _guard = ptrace::set_process_regs(pointer);
/// ...
/// // (_guard implicitly dropped)
/// ```
pub fn set_process_regs(pointer: *mut InterruptStack) -> Option<ProcessRegsGuard> {
    let contexts = context::contexts();
    let context = contexts.current()?;
    let mut context = context.write();

    let kstack = context.kstack.as_mut()?;

    context.regs = Some((kstack.as_mut_ptr() as usize, Unique::new(pointer)));
    Some(ProcessRegsGuard)
}

impl Drop for ProcessRegsGuard {
    fn drop(&mut self) {
        fn clear_process_regs() -> Option<()> {
            let contexts = context::contexts();
            let context = contexts.current()?;
            let mut context = context.write();

            context.regs = None;
            Some(())
        }
        clear_process_regs();
    }
}

/// Return the InterruptStack pointer, but relative to the specified
/// stack instead of the original.
pub unsafe fn rebase_regs_ptr(
    regs: Option<(usize, Unique<InterruptStack>)>,
    kstack: Option<&Box<[u8]>>
) -> Option<*const InterruptStack> {
    let (old_base, ptr) = regs?;
    let new_base = kstack?.as_ptr() as usize;
    Some((ptr.as_ptr() as usize - old_base + new_base) as *const _)
}
/// Return the InterruptStack pointer, but relative to the specified
/// stack instead of the original.
pub unsafe fn rebase_regs_ptr_mut(
    regs: Option<(usize, Unique<InterruptStack>)>,
    kstack: Option<&mut Box<[u8]>>
) -> Option<*mut InterruptStack> {
    let (old_base, ptr) = regs?;
    let new_base = kstack?.as_mut_ptr() as usize;
    Some((ptr.as_ptr() as usize - old_base + new_base) as *mut _)
}

/// Return a reference to the InterruptStack struct in memory. If the
/// kernel stack has been backed up by a signal handler, this instead
/// returns the struct inside that memory, as that will later be
/// restored and otherwise undo all your changes. See `update(...)` in
/// context/switch.rs.
pub unsafe fn regs_for(context: &Context) -> Option<&InterruptStack> {
    let signal_backup_regs = match context.ksig {
        None => None,
        Some((_, _, ref kstack, signum)) => {
            let is_user_handled = {
                let actions = context.actions.lock();
                signal::is_user_handled(actions[signum as usize].0.sa_handler)
            };
            if is_user_handled {
                None
            } else {
                Some(rebase_regs_ptr(context.regs, kstack.as_ref())?)
            }
        }
    };
    signal_backup_regs
        .or_else(|| context.regs.map(|regs| regs.1.as_ptr() as *const _))
        .map(|ptr| &*ptr)
}

/// Mutable version of `regs_for`
pub unsafe fn regs_for_mut(context: &mut Context) -> Option<&mut InterruptStack> {
    let signal_backup_regs = match context.ksig {
        None => None,
        Some((_, _, ref mut kstack, signum)) => {
            let is_user_handled = {
                let actions = context.actions.lock();
                signal::is_user_handled(actions[signum as usize].0.sa_handler)
            };
            if is_user_handled {
                None
            } else {
                Some(rebase_regs_ptr_mut(context.regs, kstack.as_mut())?)
            }
        }
    };
    signal_backup_regs
        .or_else(|| context.regs.map(|regs| regs.1.as_ptr()))
        .map(|ptr| &mut *ptr)
}

//  __  __
// |  \/  | ___ _ __ ___   ___  _ __ _   _
// | |\/| |/ _ \ '_ ` _ \ / _ \| '__| | | |
// | |  | |  __/ | | | | | (_) | |  | |_| |
// |_|  |_|\___|_| |_| |_|\___/|_|   \__, |
//                                   |___/

pub fn with_context_memory<F>(context: &mut Context, offset: VirtualAddress, len: usize, f: F) -> Result<()>
    where F: FnOnce(*mut u8) -> Result<()>
{
    // As far as I understand, mapping any regions following
    // USER_TMP_MISC_OFFSET is safe because no other memory location
    // is used after it. In the future it might be necessary to define
    // a maximum amount of pages that can be mapped in one batch,
    // which could be used to either internally retry `read`/`write`
    // in `proc:<pid>/mem`, or return a partial read/write.
    let start = Page::containing_address(VirtualAddress::new(crate::USER_TMP_MISC_OFFSET));

    let mut active_page_table = unsafe { ActivePageTable::new() };
    let mut target_page_table = unsafe {
        InactivePageTable::from_address(context.arch.get_page_table())
    };

    // Find the physical frames for all pages
    let mut frames = Vec::new();

    let mut result = None;
    active_page_table.with(&mut target_page_table, &mut TemporaryPage::new(start), |mapper| {
        let mut inner = || -> Result<()> {
            let start = Page::containing_address(offset);
            let end = Page::containing_address(VirtualAddress::new(offset.get() + len - 1));
            for page in Page::range_inclusive(start, end) {
                frames.push((
                    mapper.translate_page(page).ok_or(Error::new(EFAULT))?,
                    mapper.translate_page_flags(page).ok_or(Error::new(EFAULT))?
                ));
            }
            Ok(())
        };
        result = Some(inner());
    });
    result.expect("with(...) callback should always be called")?;

    // Map all the physical frames into linear pages
    let pages = frames.len();
    let mut page = start;
    let mut flusher = MapperFlushAll::new();
    for (frame, mut flags) in frames {
        flags |= EntryFlags::NO_EXECUTE | EntryFlags::WRITABLE;
        flusher.consume(active_page_table.map_to(page, frame, flags));

        page = page.next();
    }

    flusher.flush(&mut active_page_table);

    let res = f((start.start_address().get() + offset.get() % PAGE_SIZE) as *mut u8);

    // Unmap all the pages (but allow no deallocation!)
    let mut page = start;
    let mut flusher = MapperFlushAll::new();
    for _ in 0..pages {
        flusher.consume(active_page_table.unmap_return(page, true).0);
        page = page.next();
    }

    flusher.flush(&mut active_page_table);

    res
}
