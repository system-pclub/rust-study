use alloc::sync::Arc;
use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use core::alloc::{GlobalAlloc, Layout};
use core::cmp::Ordering;
use core::mem;
use spin::Mutex;

use crate::arch::{macros::InterruptStack, paging::PAGE_SIZE};
use crate::common::unique::Unique;
use crate::context::arch;
use crate::context::file::FileDescriptor;
use crate::context::memory::{Grant, Memory, SharedMemory, Tls};
use crate::ipi::{ipi, IpiKind, IpiTarget};
use crate::scheme::{SchemeNamespace, FileHandle};
use crate::sync::WaitMap;
use crate::syscall::data::SigAction;
use crate::syscall::flag::{SIG_DFL, SigActionFlags};

/// Unique identifier for a context (i.e. `pid`).
use ::core::sync::atomic::AtomicUsize;
int_like!(ContextId, AtomicContextId, usize, AtomicUsize);

/// The status of a context - used for scheduling
/// See `syscall::process::waitpid` and the `sync` module for examples of usage
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Status {
    Runnable,
    Blocked,
    Stopped(usize),
    Exited(usize)
}

#[derive(Copy, Clone, Debug)]
pub struct WaitpidKey {
    pub pid: Option<ContextId>,
    pub pgid: Option<ContextId>,
}

impl Ord for WaitpidKey {
    fn cmp(&self, other: &WaitpidKey) -> Ordering {
        // If both have pid set, compare that
        if let Some(s_pid) = self.pid {
            if let Some(o_pid) = other.pid {
                return s_pid.cmp(&o_pid);
            }
        }

        // If both have pgid set, compare that
        if let Some(s_pgid) = self.pgid {
            if let Some(o_pgid) = other.pgid {
                return s_pgid.cmp(&o_pgid);
            }
        }

        // If either has pid set, it is greater
        if self.pid.is_some() {
            return Ordering::Greater;
        }

        if other.pid.is_some() {
            return Ordering::Less;
        }

        // If either has pgid set, it is greater
        if self.pgid.is_some() {
            return Ordering::Greater;
        }

        if other.pgid.is_some() {
            return Ordering::Less;
        }

        // If all pid and pgid are None, they are equal
        Ordering::Equal
    }
}

impl PartialOrd for WaitpidKey {
    fn partial_cmp(&self, other: &WaitpidKey) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for WaitpidKey {
    fn eq(&self, other: &WaitpidKey) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for WaitpidKey {}

/// A context, which identifies either a process or a thread
#[derive(Debug)]
pub struct Context {
    /// The ID of this context
    pub id: ContextId,
    /// The group ID of this context
    pub pgid: ContextId,
    /// The ID of the parent context
    pub ppid: ContextId,
    /// The real user id
    pub ruid: u32,
    /// The real group id
    pub rgid: u32,
    /// The real namespace id
    pub rns: SchemeNamespace,
    /// The effective user id
    pub euid: u32,
    /// The effective group id
    pub egid: u32,
    /// The effective namespace id
    pub ens: SchemeNamespace,
    /// Signal mask
    pub sigmask: [u64; 2],
    /// Process umask
    pub umask: usize,
    /// Status of context
    pub status: Status,
    /// Context running or not
    pub running: bool,
    /// CPU ID, if locked
    pub cpu_id: Option<usize>,
    /// Current system call
    pub syscall: Option<(usize, usize, usize, usize, usize, usize)>,
    /// Head buffer to use when system call buffers are not page aligned
    pub syscall_head: Box<[u8]>,
    /// Tail buffer to use when system call buffers are not page aligned
    pub syscall_tail: Box<[u8]>,
    /// Context is halting parent
    pub vfork: bool,
    /// Context is being waited on
    pub waitpid: Arc<WaitMap<WaitpidKey, (ContextId, usize)>>,
    /// Context should handle pending signals
    pub pending: VecDeque<u8>,
    /// Context should wake up at specified time
    pub wake: Option<(u64, u64)>,
    /// The architecture specific context
    pub arch: arch::Context,
    /// Kernel FX - used to store SIMD and FPU registers on context switch
    pub kfx: Option<Box<[u8]>>,
    /// Kernel stack
    pub kstack: Option<Box<[u8]>>,
    /// Kernel signal backup: Registers, Kernel FX, Kernel Stack, Signal number
    pub ksig: Option<(arch::Context, Option<Box<[u8]>>, Option<Box<[u8]>>, u8)>,
    /// Restore ksig context on next switch
    pub ksig_restore: bool,
    /// Executable image
    pub image: Vec<SharedMemory>,
    /// User heap
    pub heap: Option<SharedMemory>,
    /// User stack
    pub stack: Option<SharedMemory>,
    /// User signal stack
    pub sigstack: Option<Memory>,
    /// User Thread local storage
    pub tls: Option<Tls>,
    /// User grants
    pub grants: Arc<Mutex<Vec<Grant>>>,
    /// The name of the context
    pub name: Arc<Mutex<Box<[u8]>>>,
    /// The current working directory
    pub cwd: Arc<Mutex<Vec<u8>>>,
    /// The open files in the scheme
    pub files: Arc<Mutex<Vec<Option<FileDescriptor>>>>,
    /// Signal actions
    pub actions: Arc<Mutex<Vec<(SigAction, usize)>>>,
    /// The pointer to the user-space registers, saved after certain
    /// interrupts. This pointer is somewhere inside kstack, and the
    /// kstack address at the time of creation is the first element in
    /// this tuple.
    pub regs: Option<(usize, Unique<InterruptStack>)>,
    /// A somewhat hacky way to initially stop a context when creating
    /// a new instance of the proc: scheme, entirely separate from
    /// signals or any other way to restart a process.
    pub ptrace_stop: bool
}

impl Context {
    pub fn new(id: ContextId) -> Context {
        let syscall_head = unsafe { Box::from_raw(crate::ALLOCATOR.alloc(Layout::from_size_align_unchecked(PAGE_SIZE, PAGE_SIZE)) as *mut [u8; PAGE_SIZE]) };
        let syscall_tail = unsafe { Box::from_raw(crate::ALLOCATOR.alloc(Layout::from_size_align_unchecked(PAGE_SIZE, PAGE_SIZE)) as *mut [u8; PAGE_SIZE]) };

        Context {
            id,
            pgid: id,
            ppid: ContextId::from(0),
            ruid: 0,
            rgid: 0,
            rns: SchemeNamespace::from(0),
            euid: 0,
            egid: 0,
            ens: SchemeNamespace::from(0),
            sigmask: [0; 2],
            umask: 0o022,
            status: Status::Blocked,
            running: false,
            cpu_id: None,
            syscall: None,
            syscall_head,
            syscall_tail,
            vfork: false,
            waitpid: Arc::new(WaitMap::new()),
            pending: VecDeque::new(),
            wake: None,
            arch: arch::Context::new(),
            kfx: None,
            kstack: None,
            ksig: None,
            ksig_restore: false,
            image: Vec::new(),
            heap: None,
            stack: None,
            sigstack: None,
            tls: None,
            grants: Arc::new(Mutex::new(Vec::new())),
            name: Arc::new(Mutex::new(Vec::new().into_boxed_slice())),
            cwd: Arc::new(Mutex::new(Vec::new())),
            files: Arc::new(Mutex::new(Vec::new())),
            actions: Arc::new(Mutex::new(vec![(
                SigAction {
                    sa_handler: unsafe { mem::transmute(SIG_DFL) },
                    sa_mask: [0; 2],
                    sa_flags: SigActionFlags::empty(),
                },
                0
            ); 128])),
            regs: None,
            ptrace_stop: false
        }
    }

    /// Make a relative path absolute
    /// Given a cwd of "scheme:/path"
    /// This function will turn "foo" into "scheme:/path/foo"
    /// "/foo" will turn into "scheme:/foo"
    /// "bar:/foo" will be used directly, as it is already absolute
    pub fn canonicalize(&self, path: &[u8]) -> Vec<u8> {
        let mut canon = if path.iter().position(|&b| b == b':').is_none() {
            let cwd = self.cwd.lock();

            let mut canon = if !path.starts_with(b"/") {
                let mut c = cwd.clone();
                if ! c.ends_with(b"/") {
                    c.push(b'/');
                }
                c
            } else {
                cwd[..cwd.iter().position(|&b| b == b':').map_or(1, |i| i + 1)].to_vec()
            };

            canon.extend_from_slice(&path);
            canon
        } else {
            path.to_vec()
        };

        // NOTE: assumes the scheme does not include anything like "../" or "./"
        let mut result = {
            let parts = canon.split(|&c| c == b'/')
                .filter(|&part| part != b".")
                .rev()
                .scan(0, |nskip, part| {
                    if part == b"." {
                        Some(None)
                    } else if part == b".." {
                        *nskip += 1;
                        Some(None)
                    } else if *nskip > 0 {
                            *nskip -= 1;
                            Some(None)
                    } else {
                        Some(Some(part))
                    }
                })
                .filter_map(|x| x)
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>();
            parts
                .iter()
                .rev()
                .fold(Vec::new(), |mut vec, &part| {
                    vec.extend_from_slice(part);
                    vec.push(b'/');
                    vec
                })
        };
        result.pop(); // remove extra '/'

        // replace with the root of the scheme if it's empty
        if result.is_empty() {
            let pos = canon.iter()
                            .position(|&b| b == b':')
                            .map_or(canon.len(), |p| p + 1);
            canon.truncate(pos);
            canon
        } else {
            result
        }
    }

    /// Block the context, and return true if it was runnable before being blocked
    pub fn block(&mut self) -> bool {
        if self.status == Status::Runnable {
            self.status = Status::Blocked;
            true
        } else {
            false
        }
    }

    /// Unblock context, and return true if it was blocked before being marked runnable
    pub fn unblock(&mut self) -> bool {
        if self.status == Status::Blocked {
            self.status = Status::Runnable;

            if let Some(cpu_id) = self.cpu_id {
               if cpu_id != crate::cpu_id() {
                    // Send IPI if not on current CPU
                    ipi(IpiKind::Wakeup, IpiTarget::Other);
               }
            }

            true
        } else {
            false
        }
    }

    /// Add a file to the lowest available slot.
    /// Return the file descriptor number or None if no slot was found
    pub fn add_file(&self, file: FileDescriptor) -> Option<FileHandle> {
        self.add_file_min(file, 0)
    }

    /// Add a file to the lowest available slot greater than or equal to min.
    /// Return the file descriptor number or None if no slot was found
    pub fn add_file_min(&self, file: FileDescriptor, min: usize) -> Option<FileHandle> {
        let mut files = self.files.lock();
        for (i, file_option) in files.iter_mut().enumerate() {
            if file_option.is_none() && i >= min {
                *file_option = Some(file);
                return Some(FileHandle::from(i));
            }
        }
        let len = files.len();
        if len < super::CONTEXT_MAX_FILES {
            if len >= min {
                files.push(Some(file));
                Some(FileHandle::from(len))
            } else {
                drop(files);
                self.insert_file(FileHandle::from(min), file)
            }
        } else {
            None
        }
    }

    /// Get a file
    pub fn get_file(&self, i: FileHandle) -> Option<FileDescriptor> {
        let files = self.files.lock();
        if i.into() < files.len() {
            files[i.into()].clone()
        } else {
            None
        }
    }

    /// Insert a file with a specific handle number. This is used by dup2
    /// Return the file descriptor number or None if the slot was not empty, or i was invalid
    pub fn insert_file(&self, i: FileHandle, file: FileDescriptor) -> Option<FileHandle> {
        let mut files = self.files.lock();
        if i.into() < super::CONTEXT_MAX_FILES {
            while i.into() >= files.len() {
                files.push(None);
            }
            if files[i.into()].is_none() {
                files[i.into()] = Some(file);
                Some(i)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Remove a file
    // TODO: adjust files vector to smaller size if possible
    pub fn remove_file(&self, i: FileHandle) -> Option<FileDescriptor> {
        let mut files = self.files.lock();
        if i.into() < files.len() {
            files[i.into()].take()
        } else {
            None
        }
    }
}
