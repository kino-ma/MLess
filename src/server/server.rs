use tokio::sync::mpsc;
use tonic::Status;
use tonic::{Request, Response};

use crate::proto::server_daemon_server::ServerDaemon as ServerDaemonTrait;
use crate::proto::{
    DestroyRequest, DestroyResponse, GetInfoRequest, GetInfoResponse, MonitorRequest,
    MonitorResponse, NominateRequest, NominateResponse, PingResponse, ServerState, SpawnRequest,
    SpawnResponse,
};
use crate::{RpcResult, ServerInfo};

use super::{DaemonState, DEFAULT_HOST};

#[derive(Clone, Debug)]
pub struct ServerDaemon {
    pub runtime: ServerDaemonRuntime,
    pub tx: mpsc::Sender<DaemonState>,
    pub state: DaemonState,
}

#[derive(Clone, Debug)]
pub struct ServerDaemonRuntime {
    info: ServerInfo,
}

impl ServerDaemon {
    pub fn new(tx: mpsc::Sender<DaemonState>) -> Self {
        let runtime = ServerDaemonRuntime::default();
        let state = DaemonState::Starting;

        Self { runtime, tx, state }
    }

    pub fn with_state(state: DaemonState, tx: mpsc::Sender<DaemonState>) -> Self {
        let info = state
            .get_server()
            .expect(&format!("failed to get server: invalid sate: {state:?}"))
            .clone();
        let runtime = ServerDaemonRuntime { info };

        Self { runtime, tx, state }
    }
}

#[tonic::async_trait]
impl ServerDaemonTrait for ServerDaemon {
    async fn get_info(
        &self,
        _request: Request<GetInfoRequest>,
    ) -> RpcResult<Response<GetInfoResponse>> {
        println!("GetInfo called!");

        let server = Some(self.runtime.info.clone().into());
        let state = &self.state;

        use DaemonState::*;
        let group = match &state {
            Starting | Uninitialized(_) | Failed | Joining(_) => None,
            Running(_, group) => Some(group.clone().into()),
            Authoritative(_, scheduler) => Some(
                scheduler
                    .runtime
                    .lock()
                    .unwrap()
                    .cluster
                    .group
                    .clone()
                    .into(),
            ),
        };

        let state: ServerState = state.clone().into();
        let state = state.into();

        let resposne = GetInfoResponse {
            server,
            group,
            state,
        };

        Ok(Response::new(resposne))
    }

    async fn ping(&self, _request: Request<()>) -> RpcResult<Response<PingResponse>> {
        println!("got ping!");

        let resposne = PingResponse { success: true };

        Ok(Response::new(resposne))
    }

    async fn nominate(
        &self,
        _request: Request<NominateRequest>,
    ) -> RpcResult<Response<NominateResponse>> {
        println!("got nominate!");
        Err(Status::unimplemented("not implemented"))
    }

    async fn monitor(
        &self,
        _request: Request<MonitorRequest>,
    ) -> RpcResult<Response<MonitorResponse>> {
        Ok(Response::new(MonitorResponse { windows: vec![] }))
    }
    async fn spawn(&self, _request: Request<SpawnRequest>) -> RpcResult<Response<SpawnResponse>> {
        Ok(Response::new(SpawnResponse {
            success: true,
            deployment: None,
            server: None,
        }))
    }
    async fn destroy(
        &self,
        _request: Request<DestroyRequest>,
    ) -> RpcResult<Response<DestroyResponse>> {
        Ok(Response::new(DestroyResponse { success: true }))
    }
}

impl Default for ServerDaemonRuntime {
    fn default() -> Self {
        let info = ServerInfo::new(DEFAULT_HOST);

        Self { info }
    }
}
