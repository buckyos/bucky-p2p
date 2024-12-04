
// use log::*;
use std::{
    sync::{Arc, RwLock,},
    time::Duration,
};
use async_std::{
    task
};
use futures::future::AbortRegistration;
use cyfs_base::*;
use crate::{
    types::*,
    protocol::{v0::*},
};
use crate::executor::Executor;
use crate::history::keystore::Keystore;
use crate::sn::client::{ClientManager, SnCache};
use crate::sockets::{NetListenerRef, UpdateOuterResult};
use super::{
    udp::{self, *}
};

#[derive(Clone)]
pub struct PingConfig {
    pub interval: Duration,
    pub udp: udp::Config
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SnStatus {
    Online,
    Offline
}


impl std::fmt::Display for SnStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = match self {
            Self::Online => "online",
            Self::Offline => "offline",
        };

        write!(f, "{}", v)
    }
}


impl std::str::FromStr for SnStatus {
    type Err = BuckyError;

    fn from_str(s: &str) -> BdtResult<Self> {
        match s {
            "online" => Ok(Self::Online),
            "offline" => Ok(Self::Offline),
            _ => {
                let msg = format!("unknown SnStatus value: {}", s);
                log::error!("{}", msg);

                Err(BuckyError::new(BuckyErrorCode::InvalidData, msg))
            }
        }
    }
}

#[async_trait::async_trait]
pub trait PingSession: Send + Sync + std::fmt::Display {
    fn sn(&self) -> &DeviceId;
    fn local(&self) -> Endpoint;
    fn reset(&self,  local_device: Option<Device>, sn_endpoint: Option<Endpoint>) -> Box<dyn PingSession>;
    fn clone_as_ping_session(&self) -> Box<dyn PingSession>;
    async fn wait(&self) -> BdtResult<PingSessionResp>;
    fn stop(&self);
    fn on_time_escape(&self, _now: Timestamp) {

    }
    fn on_udp_ping_resp(&self, _resp: &SnPingResp, _from: &Endpoint) -> BdtResult<()> {
        Ok(())
    }
}


enum ActiveState {
    FirstTry(Box<dyn PingSession>),
    SecondTry(Box<dyn PingSession>),
    Wait(Timestamp, Box<dyn PingSession>)
}

impl ActiveState {
    fn cur_session(&self) -> Box<dyn PingSession> {
        match self {
            Self::FirstTry(session) => session.clone_as_ping_session(),
            Self::SecondTry(session) => session.clone_as_ping_session(),
            Self::Wait(_, session) => session.clone_as_ping_session()
        }
    }
    fn trying_session(&self) -> Option<Box<dyn PingSession>> {
        match self {
            Self::FirstTry(session) => Some(session.clone_as_ping_session()),
            Self::SecondTry(session) => Some(session.clone_as_ping_session()),
            _ => None
        }
    }
}

struct ClientState {
    ipv4: Ipv4ClientState,
    ipv6: Ipv6ClientState
}

enum Ipv4ClientState {
    Init(StateWaiter),
    Connecting {
        waiter: StateWaiter,
        sessions: Vec<Box<dyn PingSession>>,
    },
    Active {
        waiter: StateWaiter,
        state: ActiveState
    },
    Timeout,
    Stopped
}

enum Ipv6ClientState {
    None,
    Try(Box<dyn PingSession>),
    Wait(Timestamp, Box<dyn PingSession>)
}

struct PingClient {
    key_store: Arc<Keystore>,
    sn_cache: Arc<SnCache>,
    config: PingConfig,
    sn_index: usize,
    sn_id: DeviceId,
    sn: Device,
    gen_seq: Arc<TempSeqGenerator>,
    net_listener: NetListenerRef,
    local_device: RwLock<Device>,
    state: RwLock<ClientState>,

}
pub type PingClientRef = Arc<PingClient>;

impl std::fmt::Display for PingClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let device_id = self.local_device.read().unwrap().desc().device_id();
        write!(f, "PingClient {{local:{}, sn:{}}}", device_id, self.sn())
    }
}

impl PingClient {
    pub(crate) fn new(
        key_store: Arc<Keystore>,
        sn_cache: Arc<SnCache>,
        config: PingConfig,
        gen_seq: Arc<TempSeqGenerator>,
        net_listener: NetListenerRef,
        sn_index: usize,
        sn: Device,
        local_device: Device,
    ) -> Arc<Self> {
        let sn_id = sn.desc().device_id();
        key_store.reset_peer(&sn_id);

        Arc::new(PingClient {
            key_store,
            sn_cache,
            config,
            gen_seq,
            net_listener,
            sn,
            sn_id,
            sn_index,
            local_device: RwLock::new(local_device),
            state: RwLock::new(ClientState {
                ipv4: Ipv4ClientState::Init(StateWaiter::new()),
                ipv6: Ipv6ClientState::None
            })
        })
    }

    pub(crate) fn reset(
        &self,
        net_listener: NetListenerRef,
        local_device: Device,
    ) -> Arc<Self> {
        Arc::new(PingClient {
            key_store,
            sn_cache,
            config,
            gen_seq,
            net_listener,
            sn,
            sn_id,
            sn_index,
            local_device: RwLock::new(local_device),
            state: RwLock::new(ClientState {
                ipv4: Ipv4ClientState::Init(StateWaiter::new()),
                ipv6: Ipv6ClientState::None
            })
        })
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self, &other)
    }

    pub fn local_device(&self) -> Device {
        self.local_device.read().unwrap().clone()
    }

    pub fn stop(&self) {
        let (waiter, sessions) = {
            let mut state = self.state.write().unwrap();
            let (waiter, mut sessions) = match &mut state.ipv4 {
                Ipv4ClientState::Init(waiter) => {
                    let waiter = waiter.transfer();
                    state.ipv4 = Ipv4ClientState::Stopped;
                    (Some(waiter), vec![])
                },
                Ipv4ClientState::Connecting {
                    waiter,
                    sessions
                } => {
                    let waiter = waiter.transfer();
                    let sessions = sessions.iter().map(|s| s.clone_as_ping_session()).collect();
                    state.ipv4 = Ipv4ClientState::Stopped;
                    (Some(waiter), sessions)
                },
                Ipv4ClientState::Active {
                    waiter,
                    state: active
                } => {
                    let waiter = waiter.transfer();
                    let sessions = if let Some(session) = active.trying_session() {
                        vec![session]
                    } else {
                        vec![]
                    };
                    state.ipv4 = Ipv4ClientState::Stopped;
                    (Some(waiter), sessions)
                },
                _ => (None, vec![])
            };

            match &mut state.ipv6 {
                Ipv6ClientState::Try(session) => {
                    sessions.push(session.clone_as_ping_session());
                    state.ipv6 = Ipv6ClientState::None
                },
                _ => {}
            }

            (waiter, sessions)
        };

        if let Some(waiter) = waiter {
            waiter.wake()
        };

        for session in sessions {
            session.stop();
        }

    }


    pub fn sn(&self) -> &DeviceId {
        &self.sn_id
    }

    pub fn index(&self) -> usize {
        self.sn_index
    }


    async fn update_local(&self, local: Endpoint, outer: Endpoint) {
        let update = self.net_listener().update_outer(&local, &outer);
        if update > UpdateOuterResult::None {
            info!("{} update local {} => {}", self, local, outer);
            let mut local_dev = self.local_device();
            let device_sn_list = local_dev.mut_connect_info().mut_sn_list();
            device_sn_list.clear();
            device_sn_list.push(self.sn().clone());

            let device_endpoints = local_dev.mut_connect_info().mut_endpoints();
            device_endpoints.clear();
            let bound_endpoints = self.net_listener().endpoints();
            for ep in bound_endpoints {
                device_endpoints.push(ep);
            }

            local_dev.body_mut().as_mut().unwrap().increase_update_time(bucky_time_now());

            let _ = sign_and_set_named_object_body(
                self.key_store.signer(),
                &mut local_dev,
                &SignatureSource::RefIndex(0),
            ).await;



            let updated = {
                let mut store = self.local_device.write().unwrap();
                if store.body().as_ref().unwrap().update_time() < local_dev.body().as_ref().unwrap().update_time() {
                    *store = local_dev;
                    true
                } else {
                    false
                }
            };

            if updated {
                if local.addr().is_ipv6() {
                    if let Ok(status) = self.wait_online().await {
                        if SnStatus::Online == status {
                            self.ping_ipv4_once();
                        }
                    }
                } else {
                    self.ping_ipv4_once();
                }
            }
        }
    }

    fn ping_ipv4_once(&self) {
        info!("{} ping once", self);
        let mut state = self.state.write().unwrap();
        match &mut state.ipv4 {
            Ipv4ClientState::Active {
                state: active,
                ..
            } => {
                match active {
                    ActiveState::Wait(_, session) => {
                        let session = session.reset(Some(self.local_device()), None);
                        *active = ActiveState::FirstTry(session.clone_as_ping_session());
                        {

                            let client = self.clone();
                            let session = session.clone_as_ping_session();
                            Executor::spawn_ok(async move {
                                client.sync_session_resp(session.as_ref(), session.wait().await);
                            });
                        }
                    },
                    _ => {}
                }
            },
            _ => {}
        }
    }

    fn sync_session_resp(&self, session: &dyn PingSession, result: BdtResult<PingSessionResp>) {
        if session.local().addr().is_ipv4() {
            self.sync_ipv4_session_resp(session, result);
        } else if session.local().addr().is_ipv6() {
            self.sync_ipv6_session_resp(session, result);
        } else {
            unreachable!()
        }
    }


    fn sync_ipv6_session_resp(&self, session: &dyn PingSession, result: BdtResult<PingSessionResp>) {
        info!("{} wait session {} finished {:?}", self, session, result);

        enum NextStep {
            None,
            Update(Endpoint, Endpoint),
        }

        let next = {
            let mut state = self.state.write().unwrap();
            match &state.ipv6 {
                Ipv6ClientState::Try(session) => {
                    let session = session.clone_as_ping_session();
                    state.ipv6 = Ipv6ClientState::Wait(bucky_time_now() + self.config.interval.as_micros() as u64, session.reset(None, None));
                    match result {
                        Ok(resp) => if resp.endpoints.len() > 0 {
                            NextStep::Update(session.local().clone(), resp.endpoints[0])
                        } else {
                            NextStep::None
                        },
                        Err(_) => NextStep::None
                    }
                },
                _ => NextStep::None,
            }
        };

        if let NextStep::Update(local, outer) = next {
            let client = self.clone();
            Executor::spawn_ok(async move {
                client.update_local(local, outer).await;
            });
        }
    }


    fn sync_ipv4_session_resp(self: &PingClientRef, session: &dyn PingSession, result: BdtResult<PingSessionResp>) {
        info!("{} wait session {} finished {:?}", self, session, result);
        struct NextStep {
            waiter: Option<StateWaiter>,
            update: Option<(Endpoint, Endpoint)>,
            to_start: Option<Box<dyn PingSession>>,
            ping_once: bool,
            update_cache: Option<Option<Endpoint>>
        }

        impl NextStep {
            fn none() -> Self {
                Self {
                    waiter: None,
                    update: None,
                    to_start: None,
                    ping_once: false,
                    update_cache: None
                }
            }
        }

        let next = {
            let mut state = self.state.write().unwrap();
            match &mut state.ipv4 {
                Ipv4ClientState::Connecting {
                    waiter,
                    sessions
                } => {
                    if let Some(index) = sessions.iter().enumerate().find_map(|(index, exists)| if exists.local() == session.local() { Some(index) } else { None }) {
                        match result {
                            Ok(resp) => {
                                let mut next = NextStep::none();
                                next.waiter = Some(waiter.transfer());

                                if resp.endpoints.len() > 0 {
                                    next.update = Some((session.local(), resp.endpoints[0]));
                                }

                                info!("{} online", self);

                                next.update_cache = Some(Some(resp.from));
                                state.ipv4 = Ipv4ClientState::Active {
                                    waiter: StateWaiter::new(),
                                    state: ActiveState::Wait(bucky_time_now() + self.config.interval.as_micros() as u64, session.reset(None, Some(resp.from)))
                                };

                                next
                            },
                            Err(_err) => {
                                sessions.remove(index);
                                let mut next = NextStep::none();
                                if sessions.len() == 0 {
                                    error!("{} timeout", self);
                                    next.waiter = Some(waiter.transfer());
                                    state.ipv4 = Ipv4ClientState::Timeout;
                                }

                                next
                            }
                        }
                    } else {
                        NextStep::none()
                    }
                },
                Ipv4ClientState::Active {
                    waiter,
                    state: active
                } => {
                    let mut next = NextStep::none();
                    if !active.cur_session().local().is_same_ip_addr(&session.local()) {
                        if let Ok(resp) = result {
                            if resp.endpoints.len() > 0 {
                                next.update = Some((session.local(), resp.endpoints[0]));
                            }
                        }
                    } else if active.trying_session().and_then(|exists| if exists.local() == session.local() { Some(()) } else { None }).is_some() {
                        match result {
                            Ok(resp) => {
                                *active = ActiveState::Wait(bucky_time_now() + self.config.interval.as_micros() as u64, session.reset(None, None));

                                if resp.endpoints.len() > 0 {
                                    next.update = Some((session.local(), resp.endpoints[0]));
                                } else if resp.err == BuckyErrorCode::NotFound {
                                    next.ping_once = true;
                                }
                            },
                            Err(_err) => {
                                match active {
                                    ActiveState::FirstTry(session) => {
                                        self.key_store.reset_peer(&self.sn());
                                        let session = session.reset(None, None);
                                        info!("{} start second try", self);
                                        *active = ActiveState::SecondTry(session.clone_as_ping_session());
                                        next.to_start = Some(session);
                                    },
                                    ActiveState::SecondTry(_) => {
                                        next.waiter = Some(waiter.transfer());
                                        error!("{} timeout", self);
                                        state.ipv4 = Ipv4ClientState::Timeout;
                                        next.update_cache = Some(None);
                                    },
                                    _ => {}
                                }
                            }
                        }
                    }
                    next
                },
                _ => NextStep::none()
            }
        };

        if let Some(update) = next.update_cache {
            if let Some(remote) = update {
                self.sn_cache.add_active(session.sn(), EndpointPair::from((session.local().clone(), remote)));
            } else {
                self.sn_cache.remove_active(session.sn());
            }
        }

        if let Some(session) = next.to_start {
            let client = self.clone();
            Executor::spawn_ok(async move {
                client.sync_session_resp(session.as_ref(), session.wait().await);
            });
        }

        if let Some(waiter) = next.waiter {
            waiter.wake();
        }

        if let Some((local, outer)) = next.update {
            let client = self.clone();
            Executor::spawn_ok(async move {
                client.update_local(local, outer).await;
            });
        } else if next.ping_once {
            self.ping_ipv4_once();
        }

    }

    pub async fn wait_offline(&self) -> BdtResult<()> {
        enum NextStep {
            Wait(AbortRegistration),
            Return(BdtResult<()>)
        }

        let next = {
            let mut state = self.state.write().unwrap();
            match &mut state.ipv4 {
                Ipv4ClientState::Stopped => NextStep::Return(Err(BuckyError::new(BuckyErrorCode::Interrupted, "user canceled"))),
                Ipv4ClientState::Active {
                    waiter,
                    ..
                } => NextStep::Wait(waiter.new_waiter()),
                Ipv4ClientState::Timeout =>  NextStep::Return(Ok(())),
                _ => NextStep::Return(Err(BuckyError::new(BuckyErrorCode::ErrorState, "not online"))),
            }
        };

        match next {
            NextStep::Return(result) => result,
            NextStep::Wait(waiter) => {
                StateWaiter::wait(waiter, || {
                    let state = self.state.read().unwrap();
                    match &state.ipv4 {
                        Ipv4ClientState::Stopped => Err(BuckyError::new(BuckyErrorCode::Interrupted, "user canceled")),
                        Ipv4ClientState::Timeout =>  Ok(()),
                        _ => unreachable!()
                    }
                }).await
            }
        }
    }

    pub async fn wait_online(self: &PingClientRef) -> BdtResult<SnStatus> {
        info!("{} waiting online", self);
        enum NextStep {
            Wait(AbortRegistration),
            Start(AbortRegistration),
            Return(BdtResult<SnStatus>)
        }
        let next = {
            let mut state = self.state.write().unwrap();
            match &mut state.ipv4 {
                Ipv4ClientState::Init(waiter) => {
                    let waiter = waiter.new_waiter();
                    NextStep::Start(waiter)
                },
                Ipv4ClientState::Connecting{ waiter, ..} => NextStep::Wait(waiter.new_waiter()),
                Ipv4ClientState::Active {..} => NextStep::Return(Ok(SnStatus::Online)),
                Ipv4ClientState::Timeout =>  NextStep::Return(Ok(SnStatus::Offline)),
                Ipv4ClientState::Stopped => NextStep::Return(Err(BuckyError::new(BuckyErrorCode::Interrupted, "user canceled"))),
            }
        };

        let state = || {
            let state = self.state.read().unwrap();
            match &state.ipv4 {
                Ipv4ClientState::Active {..} => Ok(SnStatus::Online),
                Ipv4ClientState::Timeout =>  Ok(SnStatus::Offline),
                Ipv4ClientState::Stopped => Err(BuckyError::new(BuckyErrorCode::Interrupted, "user canceled")),
                _ => unreachable!()
            }
        };

        match next {
            NextStep::Return(result) => result,
            NextStep::Wait(waiter) => StateWaiter::wait(waiter, state).await,
            NextStep::Start(waiter) => {
                info!("{} started", self);
                let mut ipv6_session = None;
                let mut ipv4_sessions = vec![];
                for local in self.net_listener.udp().iter().filter(|interface| interface.local().addr().is_ipv4()) {
                    let sn_endpoints: Vec<Endpoint> = self.sn.connect_info().endpoints().iter().filter(|endpoint| endpoint.is_udp() && endpoint.is_same_ip_version(&local.local())).cloned().collect();
                    if sn_endpoints.len() > 0 {
                        let params = UdpSesssionParams {
                            config: self.config.udp.clone(),
                            local: local.clone(),
                            local_device: self.local_device(),
                            with_device: true,
                            sn_desc: self.sn.desc().clone(),
                            sn_endpoints,
                        };
                        let session = UdpPingSession::new(self.key_store.clone(), self.gen_seq.clone(), params).clone_as_ping_session();

                        info!("{} add session {}", self, session);
                        ipv4_sessions.push(session);
                    }
                };


                for local in self.net_listener.udp().iter().filter(|interface| interface.local().addr().is_ipv6()) {
                    let sn_endpoints: Vec<Endpoint> = self.sn.connect_info().endpoints().iter().filter(|endpoint| endpoint.is_udp() && endpoint.is_same_ip_version(&local.local())).cloned().collect();
                    if sn_endpoints.len() > 0 {
                        let params = UdpSesssionParams {
                            config: self.config.udp.clone(),
                            local: local.clone(),
                            local_device: self.local_device(),
                            with_device: false,
                            sn_desc: self.sn.desc().clone(),
                            sn_endpoints,
                        };
                        let session = UdpPingSession::new(self.key_store.clone(), self.gen_seq.clone(), params).clone_as_ping_session();

                        info!("{} add session {}", self, session);
                        ipv6_session = Some(session);
                        break;
                    }
                };

                let next = {
                    let mut state = self.state.write().unwrap();
                    match &mut state.ipv4 {
                        Ipv4ClientState::Init(waiter) => {
                            let waiter = waiter.transfer();
                            if ipv4_sessions.len() > 0 {
                                state.ipv4 = Ipv4ClientState::Connecting {
                                    waiter,
                                    sessions: ipv4_sessions.iter().map(|s| s.clone_as_ping_session()).collect(),
                                };
                                if let Some(session) = ipv6_session.as_ref() {
                                    state.ipv6 = Ipv6ClientState::Try(session.clone_as_ping_session());
                                }
                                Ok(true)
                            } else {
                                state.ipv4 = Ipv4ClientState::Stopped;
                                Err((BuckyError::new(BuckyErrorCode::Interrupted, "no bound endpoint"), waiter))
                            }
                        },
                        _ => Ok(false)
                    }
                };

                match next {
                    Ok(start) => {
                        if start {
                            for session in ipv4_sessions.into_iter() {
                                let client = self.clone();
                                Executor::spawn_ok(async move {
                                    client.sync_session_resp(session.as_ref(), session.wait().await);
                                });
                            }
                            if let Some(session) = ipv6_session {
                                let client = self.clone();
                                Executor::spawn_ok(async move {
                                    client.sync_session_resp(session.as_ref(), session.wait().await);
                                });
                            }
                        }
                        StateWaiter::wait(waiter, state).await
                    },
                    Err((err, waiter)) => {
                        waiter.wake();
                        Err(err)
                    }
                }
            }
        }

    }

    pub fn on_time_escape(self: &PingClientRef, now: Timestamp) {
        let sessions = {
            let mut state = self.state.write().unwrap();
            let mut sessions = match &mut state.ipv4 {
                Ipv4ClientState::Connecting {
                    sessions,
                    ..
                } => sessions.iter().map(|session| session.clone_as_ping_session()).collect(),
                Ipv4ClientState::Active {
                    state: active,
                    ..
                } => {
                    match active {
                        ActiveState::Wait(next_time, session) => {
                            if now > *next_time {
                                let session = session.clone_as_ping_session();
                                *active = ActiveState::FirstTry(session.clone_as_ping_session());
                                {

                                    let client = self.clone();
                                    let session = session.clone_as_ping_session();
                                    Executor::spawn_ok(async move {
                                        client.sync_session_resp(session.as_ref(), session.wait().await);
                                    });
                                }
                                vec![session]
                            } else {
                                vec![]
                            }
                        },
                        ActiveState::FirstTry(session) => vec![session.clone_as_ping_session()],
                        ActiveState::SecondTry(session) => vec![session.clone_as_ping_session()],
                    }
                },
                _ => vec![]
            };

            match &mut state.ipv6 {
                Ipv6ClientState::Try(session) => {
                    sessions.push(session.clone_as_ping_session());
                },
                Ipv6ClientState::Wait(next_time, session) => {
                    if now > *next_time {
                        let session = session.clone_as_ping_session();
                        state.ipv6 = Ipv6ClientState::Try(session.clone_as_ping_session());
                        sessions.push(session.clone_as_ping_session());
                        {
                            let client = self.clone();
                            let session = session.clone_as_ping_session();
                            Executor::spawn_ok(async move {
                                client.sync_session_resp(session.as_ref(), session.wait().await);
                            });
                        }
                    }
                },
                _ => {}
            }

            sessions
        };

        for session in sessions {
            session.on_time_escape(now);
        }
    }

    pub fn on_udp_ping_resp(&self, resp: &SnPingResp, from: &Endpoint, interface: Interface) {
        let sessions = {
            let state = self.state.read().unwrap();

            if from.addr().is_ipv4() {
                match &state.ipv4 {
                    Ipv4ClientState::Connecting {
                        sessions,
                        ..
                    } => sessions.iter().filter_map(|session| {
                        if session.local() == interface.local() {
                            Some(session.clone_as_ping_session())
                        } else {
                            None
                        }
                    }).collect(),
                    Ipv4ClientState::Active {
                        state: active,
                        ..
                    } => {
                        if let Some(session) = active.trying_session().and_then(|session| if session.local() == interface.local() { Some(session) } else { None }) {
                            vec![session]
                        } else {
                            vec![]
                        }
                    },
                    _ => vec![]
                }
            } else {
                match &state.ipv6 {
                    Ipv6ClientState::Try(session) => if session.local() == interface.local() {
                        vec![session.clone_as_ping_session()]
                    } else {
                        vec![]
                    },
                    _ => vec![]
                }
            }
        };

        for session in sessions {
            let _ = session.on_udp_ping_resp(resp, from);
        }
    }


}




