/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::devtools;
use crate::dom::abstractworker::WorkerScriptMsg;
use crate::dom::abstractworkerglobalscope::{run_worker_event_loop, WorkerEventLoopMethods};
use crate::dom::bindings::codegen::Bindings::ServiceWorkerGlobalScopeBinding;
use crate::dom::bindings::codegen::Bindings::ServiceWorkerGlobalScopeBinding::ServiceWorkerGlobalScopeMethods;
use crate::dom::bindings::codegen::Bindings::WorkerBinding::WorkerType;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::root::{DomRoot, RootCollection, ThreadLocalStackRoots};
use crate::dom::bindings::str::DOMString;
use crate::dom::bindings::structuredclone;
use crate::dom::dedicatedworkerglobalscope::AutoWorkerReset;
use crate::dom::event::Event;
use crate::dom::eventtarget::EventTarget;
use crate::dom::extendableevent::ExtendableEvent;
use crate::dom::extendablemessageevent::ExtendableMessageEvent;
use crate::dom::globalscope::GlobalScope;
use crate::dom::messageevent::MessageEvent;
use crate::dom::worker::TrustedWorkerAddress;
use crate::dom::workerglobalscope::WorkerGlobalScope;
use crate::fetch::load_whole_resource;
use crate::realms::{enter_realm, AlreadyInRealm, InRealm};
use crate::script_runtime::{
    new_rt_and_cx, CommonScriptMsg, JSContext as SafeJSContext, Runtime, ScriptChan,
};
use crate::task_queue::{QueuedTask, QueuedTaskConversion, TaskQueue};
use crate::task_source::TaskSourceName;
use crossbeam_channel::{after, unbounded, Receiver, Sender};
use devtools_traits::DevtoolScriptControlMsg;
use dom_struct::dom_struct;
use ipc_channel::ipc::{IpcReceiver, IpcSender};
use ipc_channel::router::ROUTER;
use js::jsapi::{JSContext, JS_AddInterruptCallback};
use js::jsval::UndefinedValue;
use msg::constellation_msg::PipelineId;
use net_traits::request::{CredentialsMode, Destination, ParserMetadata, Referrer, RequestBuilder};
use net_traits::{CustomResponseMediator, IpcSend};
use script_traits::{ScopeThings, ServiceWorkerMsg, WorkerGlobalScopeInit, WorkerScriptLoadOrigin};
use servo_config::pref;
use servo_rand::random;
use servo_url::ServoUrl;
use std::thread;
use std::time::{Duration, Instant};
use style::thread_state::{self, ThreadState};

/// Messages used to control service worker event loop
pub enum ServiceWorkerScriptMsg {
    /// Message common to all workers
    CommonWorker(WorkerScriptMsg),
    /// Message to request a custom response by the service worker
    Response(CustomResponseMediator),
    /// Wake-up call from the task queue.
    WakeUp,
}

impl QueuedTaskConversion for ServiceWorkerScriptMsg {
    fn task_source_name(&self) -> Option<&TaskSourceName> {
        let script_msg = match self {
            ServiceWorkerScriptMsg::CommonWorker(WorkerScriptMsg::Common(script_msg)) => script_msg,
            _ => return None,
        };
        match script_msg {
            CommonScriptMsg::Task(_category, _boxed, _pipeline_id, task_source) => {
                Some(&task_source)
            },
            _ => None,
        }
    }

    fn pipeline_id(&self) -> Option<PipelineId> {
        // Workers always return None, since the pipeline_id is only used to check for document activity,
        // and this check does not apply to worker event-loops.
        None
    }

    fn into_queued_task(self) -> Option<QueuedTask> {
        let script_msg = match self {
            ServiceWorkerScriptMsg::CommonWorker(WorkerScriptMsg::Common(script_msg)) => script_msg,
            _ => return None,
        };
        let (category, boxed, pipeline_id, task_source) = match script_msg {
            CommonScriptMsg::Task(category, boxed, pipeline_id, task_source) => {
                (category, boxed, pipeline_id, task_source)
            },
            _ => return None,
        };
        Some((None, category, boxed, pipeline_id, task_source))
    }

    fn from_queued_task(queued_task: QueuedTask) -> Self {
        let (_worker, category, boxed, pipeline_id, task_source) = queued_task;
        let script_msg = CommonScriptMsg::Task(category, boxed, pipeline_id, task_source);
        ServiceWorkerScriptMsg::CommonWorker(WorkerScriptMsg::Common(script_msg))
    }

    fn inactive_msg() -> Self {
        // Inactive is only relevant in the context of a browsing-context event-loop.
        panic!("Workers should never receive messages marked as inactive");
    }

    fn wake_up_msg() -> Self {
        ServiceWorkerScriptMsg::WakeUp
    }

    fn is_wake_up(&self) -> bool {
        match self {
            ServiceWorkerScriptMsg::WakeUp => true,
            _ => false,
        }
    }
}

pub enum MixedMessage {
    FromServiceWorker(ServiceWorkerScriptMsg),
    FromDevtools(DevtoolScriptControlMsg),
}

#[derive(Clone, JSTraceable)]
pub struct ServiceWorkerChan {
    pub sender: Sender<ServiceWorkerScriptMsg>,
}

impl ScriptChan for ServiceWorkerChan {
    fn send(&self, msg: CommonScriptMsg) -> Result<(), ()> {
        self.sender
            .send(ServiceWorkerScriptMsg::CommonWorker(
                WorkerScriptMsg::Common(msg),
            ))
            .map_err(|_| ())
    }

    fn clone(&self) -> Box<dyn ScriptChan + Send> {
        Box::new(ServiceWorkerChan {
            sender: self.sender.clone(),
        })
    }
}

unsafe_no_jsmanaged_fields!(TaskQueue<ServiceWorkerScriptMsg>);

#[dom_struct]
pub struct ServiceWorkerGlobalScope {
    workerglobalscope: WorkerGlobalScope,

    #[ignore_malloc_size_of = "Defined in std"]
    task_queue: TaskQueue<ServiceWorkerScriptMsg>,

    #[ignore_malloc_size_of = "Defined in std"]
    own_sender: Sender<ServiceWorkerScriptMsg>,

    /// A port on which a single "time-out" message can be received,
    /// indicating the sw should stop running,
    /// while still draining the task-queue
    // and running all enqueued, and not cancelled, tasks.
    #[ignore_malloc_size_of = "Defined in std"]
    time_out_port: Receiver<Instant>,

    #[ignore_malloc_size_of = "Defined in std"]
    swmanager_sender: IpcSender<ServiceWorkerMsg>,

    scope_url: ServoUrl,
}

impl WorkerEventLoopMethods for ServiceWorkerGlobalScope {
    type WorkerMsg = ServiceWorkerScriptMsg;
    type Event = MixedMessage;

    fn task_queue(&self) -> &TaskQueue<ServiceWorkerScriptMsg> {
        &self.task_queue
    }

    fn handle_event(&self, event: MixedMessage) {
        self.handle_mixed_message(event);
    }

    fn handle_worker_post_event(&self, _worker: &TrustedWorkerAddress) -> Option<AutoWorkerReset> {
        None
    }

    fn from_worker_msg(&self, msg: ServiceWorkerScriptMsg) -> MixedMessage {
        MixedMessage::FromServiceWorker(msg)
    }

    fn from_devtools_msg(&self, msg: DevtoolScriptControlMsg) -> MixedMessage {
        MixedMessage::FromDevtools(msg)
    }
}

impl ServiceWorkerGlobalScope {
    fn new_inherited(
        init: WorkerGlobalScopeInit,
        worker_url: ServoUrl,
        from_devtools_receiver: Receiver<DevtoolScriptControlMsg>,
        runtime: Runtime,
        own_sender: Sender<ServiceWorkerScriptMsg>,
        receiver: Receiver<ServiceWorkerScriptMsg>,
        time_out_port: Receiver<Instant>,
        swmanager_sender: IpcSender<ServiceWorkerMsg>,
        scope_url: ServoUrl,
    ) -> ServiceWorkerGlobalScope {
        ServiceWorkerGlobalScope {
            workerglobalscope: WorkerGlobalScope::new_inherited(
                init,
                DOMString::new(),
                WorkerType::Classic, // FIXME(cybai): Should be provided from `Run Service Worker`
                worker_url,
                runtime,
                from_devtools_receiver,
                None,
            ),
            task_queue: TaskQueue::new(receiver, own_sender.clone()),
            own_sender: own_sender,
            time_out_port,
            swmanager_sender: swmanager_sender,
            scope_url: scope_url,
        }
    }

    #[allow(unsafe_code)]
    pub fn new(
        init: WorkerGlobalScopeInit,
        worker_url: ServoUrl,
        from_devtools_receiver: Receiver<DevtoolScriptControlMsg>,
        runtime: Runtime,
        own_sender: Sender<ServiceWorkerScriptMsg>,
        receiver: Receiver<ServiceWorkerScriptMsg>,
        time_out_port: Receiver<Instant>,
        swmanager_sender: IpcSender<ServiceWorkerMsg>,
        scope_url: ServoUrl,
    ) -> DomRoot<ServiceWorkerGlobalScope> {
        let cx = runtime.cx();
        let scope = Box::new(ServiceWorkerGlobalScope::new_inherited(
            init,
            worker_url,
            from_devtools_receiver,
            runtime,
            own_sender,
            receiver,
            time_out_port,
            swmanager_sender,
            scope_url,
        ));
        unsafe { ServiceWorkerGlobalScopeBinding::Wrap(SafeJSContext::from_ptr(cx), scope) }
    }

    #[allow(unsafe_code)]
    // https://html.spec.whatwg.org/multipage/#run-a-worker
    pub fn run_serviceworker_scope(
        scope_things: ScopeThings,
        own_sender: Sender<ServiceWorkerScriptMsg>,
        receiver: Receiver<ServiceWorkerScriptMsg>,
        devtools_receiver: IpcReceiver<DevtoolScriptControlMsg>,
        swmanager_sender: IpcSender<ServiceWorkerMsg>,
        scope_url: ServoUrl,
    ) {
        let ScopeThings {
            script_url,
            init,
            worker_load_origin,
            ..
        } = scope_things;

        let serialized_worker_url = script_url.to_string();
        let origin = GlobalScope::current()
            .expect("No current global object")
            .origin()
            .immutable()
            .clone();
        thread::Builder::new()
            .name(format!("ServiceWorker for {}", serialized_worker_url))
            .spawn(move || {
                thread_state::initialize(ThreadState::SCRIPT | ThreadState::IN_WORKER);
                let roots = RootCollection::new();
                let _stack_roots = ThreadLocalStackRoots::new(&roots);

                let WorkerScriptLoadOrigin {
                    referrer_url,
                    referrer_policy,
                    pipeline_id,
                } = worker_load_origin;

                let referrer = referrer_url.map(|referrer_url| Referrer::ReferrerUrl(referrer_url));

                let request = RequestBuilder::new(script_url.clone())
                    .destination(Destination::ServiceWorker)
                    .credentials_mode(CredentialsMode::Include)
                    .parser_metadata(ParserMetadata::NotParserInserted)
                    .use_url_credentials(true)
                    .pipeline_id(Some(pipeline_id))
                    .referrer(referrer)
                    .referrer_policy(referrer_policy)
                    .origin(origin);

                let (url, source) = match load_whole_resource(
                    request,
                    &init.resource_threads.sender(),
                    &GlobalScope::current().expect("No current global object"),
                ) {
                    Err(_) => {
                        println!("error loading script {}", serialized_worker_url);
                        return;
                    },
                    Ok((metadata, bytes)) => {
                        (metadata.final_url, String::from_utf8(bytes).unwrap())
                    },
                };

                let runtime = new_rt_and_cx(None);

                let (devtools_mpsc_chan, devtools_mpsc_port) = unbounded();
                ROUTER
                    .route_ipc_receiver_to_crossbeam_sender(devtools_receiver, devtools_mpsc_chan);

                // Service workers are time limited
                // https://w3c.github.io/ServiceWorker/#service-worker-lifetime
                let sw_lifetime_timeout = pref!(dom.serviceworker.timeout_seconds) as u64;
                let time_out_port = after(Duration::new(sw_lifetime_timeout, 0));

                let global = ServiceWorkerGlobalScope::new(
                    init,
                    url,
                    devtools_mpsc_port,
                    runtime,
                    own_sender,
                    receiver,
                    time_out_port,
                    swmanager_sender,
                    scope_url,
                );
                let scope = global.upcast::<WorkerGlobalScope>();

                unsafe {
                    // Handle interrupt requests
                    JS_AddInterruptCallback(*scope.get_cx(), Some(interrupt_callback));
                }

                scope.execute_script(DOMString::from(source));

                global.dispatch_activate();
                let reporter_name = format!("service-worker-reporter-{}", random::<u64>());
                scope
                    .upcast::<GlobalScope>()
                    .mem_profiler_chan()
                    .run_with_memory_reporting(
                        || {
                            // Step 29, Run the responsible event loop specified
                            // by inside settings until it is destroyed.
                            // The worker processing model remains on this step
                            // until the event loop is destroyed,
                            // which happens after the closing flag is set to true,
                            // or until the worker has run beyond its allocated time.
                            while !scope.is_closing() || !global.has_timed_out() {
                                run_worker_event_loop(&*global, None);
                            }
                        },
                        reporter_name,
                        scope.script_chan(),
                        CommonScriptMsg::CollectReports,
                    );
            })
            .expect("Thread spawning failed");
    }

    fn handle_mixed_message(&self, msg: MixedMessage) -> bool {
        match msg {
            MixedMessage::FromDevtools(msg) => {
                match msg {
                    DevtoolScriptControlMsg::EvaluateJS(_pipe_id, string, sender) => {
                        devtools::handle_evaluate_js(self.upcast(), string, sender)
                    },
                    DevtoolScriptControlMsg::WantsLiveNotifications(_pipe_id, bool_val) => {
                        devtools::handle_wants_live_notifications(self.upcast(), bool_val)
                    },
                    _ => debug!("got an unusable devtools control message inside the worker!"),
                }
                true
            },
            MixedMessage::FromServiceWorker(msg) => {
                self.handle_script_event(msg);
                true
            },
        }
    }

    fn has_timed_out(&self) -> bool {
        // Note: this should be included in the `select` inside `run_worker_event_loop`,
        // otherwise a block on the select can prevent the timeout.
        if self.time_out_port.try_recv().is_ok() {
            let _ = self
                .swmanager_sender
                .send(ServiceWorkerMsg::Timeout(self.scope_url.clone()));
            return true;
        }
        false
    }

    fn handle_script_event(&self, msg: ServiceWorkerScriptMsg) {
        use self::ServiceWorkerScriptMsg::*;

        match msg {
            CommonWorker(WorkerScriptMsg::DOMMessage { data, .. }) => {
                let scope = self.upcast::<WorkerGlobalScope>();
                let target = self.upcast();
                let _ac = enter_realm(&*scope);
                rooted!(in(*scope.get_cx()) let mut message = UndefinedValue());
                if let Ok(ports) = structuredclone::read(scope.upcast(), data, message.handle_mut())
                {
                    ExtendableMessageEvent::dispatch_jsval(
                        target,
                        scope.upcast(),
                        message.handle(),
                        ports,
                    );
                } else {
                    MessageEvent::dispatch_error(target, scope.upcast());
                }
            },
            CommonWorker(WorkerScriptMsg::Common(msg)) => {
                self.upcast::<WorkerGlobalScope>().process_event(msg);
            },
            Response(mediator) => {
                // TODO XXXcreativcoder This will eventually use a FetchEvent interface to fire event
                // when we have the Request and Response dom api's implemented
                // https://w3c.github.io/ServiceWorker/#fetchevent-interface
                self.upcast::<EventTarget>().fire_event(atom!("fetch"));
                let _ = mediator.response_chan.send(None);
            },
            WakeUp => {},
        }
    }

    pub fn script_chan(&self) -> Box<dyn ScriptChan + Send> {
        Box::new(ServiceWorkerChan {
            sender: self.own_sender.clone(),
        })
    }

    fn dispatch_activate(&self) {
        let event = ExtendableEvent::new(self, atom!("activate"), false, false);
        let event = (&*event).upcast::<Event>();
        self.upcast::<EventTarget>().dispatch_event(event);
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn interrupt_callback(cx: *mut JSContext) -> bool {
    let in_realm_proof = AlreadyInRealm::assert_for_cx(SafeJSContext::from_ptr(cx));
    let global = GlobalScope::from_context(cx, InRealm::Already(&in_realm_proof));
    let worker =
        DomRoot::downcast::<WorkerGlobalScope>(global).expect("global is not a worker scope");
    assert!(worker.is::<ServiceWorkerGlobalScope>());

    // A false response causes the script to terminate
    !worker.is_closing()
}

impl ServiceWorkerGlobalScopeMethods for ServiceWorkerGlobalScope {
    // https://w3c.github.io/ServiceWorker/#dom-serviceworkerglobalscope-onmessage
    event_handler!(message, GetOnmessage, SetOnmessage);

    // https://w3c.github.io/ServiceWorker/#dom-serviceworkerglobalscope-onmessageerror
    event_handler!(messageerror, GetOnmessageerror, SetOnmessageerror);
}
