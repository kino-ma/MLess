pub mod interface;
pub mod mean;
pub mod stats;
pub mod uninit;

use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tonic::transport::Channel;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::proto::scheduler_client::SchedulerClient;
use crate::proto::scheduler_server::Scheduler;
use crate::proto::server_daemon_client::ServerDaemonClient;
use crate::proto::{
    ClusterState, DeployRequest, DeployResponse, Deployment, JoinRequest, JoinResponse,
    LookupRequest, LookupResponse, NominateRequest, Nomination, NotifyRequest, NotifyResponse,
    ReportRequest, ReportResponse, Server, SpawnRequest, SpawnResponse,
};
use crate::utils::IdMap;
use crate::{AppInstanceMap, AppInstancesInfo, DeploymentInfo, GroupInfo, RpcResult, ServerInfo};
use crate::{Error, Result};

use self::interface::DeploymentScheduler;
use self::stats::{ServerStats, StatsMap};

#[derive(Debug)]
pub struct AuthoritativeScheduler {
    pub runtime: Arc<Mutex<SchedulerRuntime>>,
}

#[derive(Clone, Debug)]
pub struct SchedulerRuntime {
    pub cluster: Cluster,
    pub other: Cluster,
    pub scheduler: Box<dyn DeploymentScheduler>,
    pub deployments: IdMap<DeploymentInfo>,
}

#[derive(Clone, Debug)]
pub struct Cluster {
    pub group: GroupInfo,
    pub servers: Vec<ServerInfo>,
    pub instances: AppInstanceMap,
    pub server_stats: StatsMap,
}

impl AuthoritativeScheduler {
    pub fn new(cluster: Cluster, other: Cluster, scheduler: Box<dyn DeploymentScheduler>) -> Self {
        let runtime = Arc::new(Mutex::new(SchedulerRuntime {
            cluster,
            other,
            scheduler,
            deployments: IdMap::new(),
        }));

        Self { runtime }
    }

    pub fn push_server(&self, server: ServerInfo) {
        self.runtime.lock().unwrap().cluster.servers.push(server);
    }

    pub async fn deploy_to_other(&self, request: DeployRequest) -> Result<DeployResponse> {
        let mut client = self.other_client().await?;
        let request = Request::new(request);

        let response = client.deploy(request).await?;
        if !response.get_ref().success {
            return Err("Unsuccessful deploy".into());
        }

        println!(
            "Successfully deployed to the other group: {:?}",
            response.get_ref().deployment
        );

        Ok(response.into_inner())
    }

    pub async fn notify_to_other(&self) -> Result<()> {
        let mut client = self.other_client().await?;

        let runtime = SchedulerRuntime::clone_inner(&self.runtime);
        let cluster = Some(runtime.cluster.try_into()?);
        let request = Request::new(NotifyRequest { cluster });

        let response = client.notify(request).await?;
        if !response.get_ref().success {
            return Err("Unsuccessful notify".into());
        }

        println!("Successfully notified to the other group");

        Ok(())
    }

    pub async fn deploy_in_us(&self, deployment: DeploymentInfo) -> Result<SpawnResponse> {
        let request = SpawnRequest {
            deployment: Some(deployment.clone().into()),
        };

        let target_server = {
            println!("taking lock of runtime to get scheduler and stats");
            let runtime = self.runtime.lock().unwrap();
            println!("took lock");

            runtime
                .scheduler
                .schedule(&runtime.cluster.server_stats)
                .unwrap_or({
                    println!("WARN: failed to schedule. Using the first server");
                    runtime.cluster.servers[0].clone()
                })
        };
        println!("got target server = {:?}", &target_server);

        let mut client = self.client(&target_server).await?;

        let request = Request::new(request);

        let response = client.spawn(request).await?;
        if !response.get_ref().success {
            return Err("Unsuccessful spawn".into());
        }

        println!("spawend successfully");

        // Update deployments information and instances information atomicly
        {
            println!("taking lock of runtime");
            let mut runtime = self.runtime.lock().unwrap();
            runtime
                .deployments
                .0
                .insert(deployment.id, deployment.clone());

            runtime
                .cluster
                .insert_instance(deployment, vec![target_server]);
            println!("freeing runtime");
        }

        println!(
            "Successfully spawned app ({:?}) on {:?}",
            response.get_ref().deployment,
            response.get_ref().server,
        );

        Ok(response.into_inner())
    }

    pub async fn nominate_other_scheduler(&self) -> Result<()> {
        let other_cluster = self.runtime.lock().unwrap().other.clone();
        let nomination = Some(other_cluster.to_nomination());
        let new_scheduler = &other_cluster.group.scheduler_info;

        let mut client = self.client(new_scheduler).await?;

        let request = NominateRequest { nomination };
        let resp = client.nominate(Request::new(request)).await?.into_inner();

        if !resp.success {
            return Err(Error::Text(
                "unsuccessful nomination of a new scheduler".to_owned(),
            ));
        }

        Ok(())
    }

    pub async fn client(&self, server: &ServerInfo) -> Result<ServerDaemonClient<Channel>> {
        Ok(ServerDaemonClient::connect(server.addr.clone()).await?)
    }

    pub async fn other_client(&self) -> Result<SchedulerClient<Channel>> {
        let other_addr = self.runtime.lock().unwrap().other.get_addr().to_owned();
        println!("creating other_client ({})", other_addr);

        return Ok(SchedulerClient::connect(other_addr).await?);
    }

    pub fn clone_inner(&self) -> SchedulerRuntime {
        SchedulerRuntime::clone_inner(&self.runtime)
    }
}

#[tonic::async_trait]
impl Scheduler for AuthoritativeScheduler {
    async fn join(&self, request: Request<JoinRequest>) -> RpcResult<Response<JoinResponse>> {
        println!("join() called!!");

        let server = request
            .get_ref()
            .server
            .clone()
            .expect("server cannot be None")
            .try_into()
            .map_err(<Error as Into<Status>>::into)?;

        self.push_server(server);

        let notify_err = self.notify_to_other().await;

        match notify_err {
            Err(Error::TransportError(e)) => self.nominate_other_scheduler().await,
            Err(e) => Err(e),
            Ok(_) => Ok(()),
        }
        .map_err(<Error as Into<Status>>::into)?;

        let group = Some(self.runtime.lock().unwrap().cluster.group.clone().into());

        Ok(Response::new(JoinResponse {
            success: true,
            group,
            is_scheduler: false,
            our_group: None,
            nomination: None,
        }))
    }

    async fn notify(&self, request: Request<NotifyRequest>) -> RpcResult<Response<NotifyResponse>> {
        println!("notify() called!!");

        let NotifyRequest { cluster: state } = request.into_inner();
        let state = state.expect("State cannot be None");

        let mut lock = self.runtime.lock().unwrap();
        let runtime = lock.borrow_mut();
        runtime.other = state.try_into().map_err(Status::aborted)?;

        Ok(Response::new(NotifyResponse { success: true }))
    }

    async fn report(&self, request: Request<ReportRequest>) -> RpcResult<Response<ReportResponse>> {
        // println!("report() called!!");

        let ReportRequest { server, windows } = request.into_inner();

        let server: Server = server.ok_or(Status::aborted("server cannot be empty"))?;
        let server = ServerInfo::try_from(server);
        let server = server.map_err(|e| <Error as Into<Status>>::into(e))?;

        let stats = ServerStats::from_stats(server, windows);

        let mut lock = self.runtime.lock().unwrap();
        let runtime = lock.borrow_mut();

        runtime.cluster.insert_stats(stats);

        Ok(Response::new(ReportResponse { success: true }))
    }

    async fn deploy(&self, request: Request<DeployRequest>) -> RpcResult<Response<DeployResponse>> {
        println!("deploy() called!!");

        let DeployRequest {
            source,
            authoritative,
        } = request.into_inner();

        let deployment_info = DeploymentInfo::new(source.clone());
        let deployment: Deployment = deployment_info.clone().into();
        println!("created info");

        let mut success = true;

        let resp = self.deploy_in_us(deployment_info).await;
        let resp = resp.map_err(<Error as Into<Status>>::into)?;
        success &= resp.success;
        println!("got resp");

        if authoritative {
            let resp = self
                .deploy_to_other(DeployRequest {
                    source,
                    authoritative: false,
                })
                .await
                .map_err(<Error as Into<Status>>::into)?;

            success &= resp.success;
        }
        println!("notified");

        Ok(Response::new(DeployResponse {
            success,
            deployment: Some(deployment),
        }))
    }

    async fn lookup(&self, request: Request<LookupRequest>) -> RpcResult<Response<LookupResponse>> {
        let runtime = self.clone_inner();

        let id = Uuid::parse_str(&request.get_ref().deployment_id)
            .map_err(|e| Status::aborted(e.to_string()))?;

        let server_ids = runtime
            .cluster
            .get_instance_server_ids(&id)
            .map_err(Status::aborted)?;

        let stats_map = runtime.cluster.server_stats.clone_by_ids(&server_ids);

        let target = runtime
            .scheduler
            .schedule(&stats_map)
            .ok_or(Status::aborted("Failed to schedule"))?
            .clone();

        Ok(Response::new(LookupResponse {
            success: true,
            deployment_id: id.to_string(),
            server: Some(target.into()),
        }))
    }
}

impl Clone for AuthoritativeScheduler {
    fn clone(&self) -> Self {
        let runtime = SchedulerRuntime::clone_inner(&self.runtime);
        Self {
            runtime: Arc::new(Mutex::new(runtime)),
        }
    }
}

impl SchedulerRuntime {
    pub fn new(
        this_server: &ServerInfo,
        other_server: &ServerInfo,
        scheduler: Box<dyn DeploymentScheduler>,
    ) -> Self {
        let cluster = Cluster::new(this_server);
        let other = Cluster::new(other_server);

        Self {
            cluster,
            other,
            scheduler,
            deployments: IdMap::new(),
        }
    }

    pub fn wrap(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }

    pub fn clone_inner(arc: &Arc<Mutex<Self>>) -> Self {
        println!("WARN: cloning SchedulerRuntime");
        arc.lock().unwrap().clone()
    }
}

impl Cluster {
    pub fn new(scheduler: &ServerInfo) -> Self {
        let group = GroupInfo::new(scheduler);
        let servers = vec![scheduler.clone()];
        let instances = AppInstanceMap::new();
        let server_stats = StatsMap::new();

        Self {
            group,
            servers,
            instances,
            server_stats,
        }
    }

    pub fn with_group(group: &GroupInfo) -> Self {
        let group = group.clone();
        let servers = vec![group.scheduler_info.clone()];
        let instances = AppInstanceMap::new();
        let server_stats = StatsMap::new();

        Self {
            group,
            servers,
            instances,
            server_stats,
        }
    }

    pub fn next_cluster(&self, scheduler_info: &ServerInfo) -> Self {
        let number = self.group.number + 1;
        let other_group = GroupInfo::with_number(scheduler_info, number);
        Self::with_group(&other_group)
    }

    pub fn to_nomination(&self) -> Nomination {
        let cluster = Some((*self).into());
        Nomination { cluster }
    }

    pub fn insert_instance(&mut self, deployment: DeploymentInfo, servers: Vec<ServerInfo>) {
        let id = deployment.id;
        self.instances
            .0
            .entry(id)
            .and_modify(|i| i.servers.append(&mut servers.clone()))
            .or_insert(AppInstancesInfo {
                deployment,
                servers,
            });
    }

    pub fn insert_stats(&mut self, stats: ServerStats) {
        let id = stats.server.id;
        self.server_stats
            .0
            .entry(id)
            .and_modify(|s| s.append(stats.stats.clone()))
            .or_insert(stats);
    }

    pub fn get_instance_server_ids(&self, deployment_id: &Uuid) -> Result<Vec<Uuid>> {
        println!("instances = {:?}", self.instances.0);

        Ok(self
            .instances
            .0
            .get(deployment_id)
            .ok_or("Deployment not found".to_string())?
            .servers
            .iter()
            .map(|s| s.id)
            .collect())
    }

    pub fn get_addr(&self) -> &str {
        &self.group.scheduler_info.addr
    }
}

impl Into<ClusterState> for Cluster {
    fn into(self) -> ClusterState {
        let group = Some(self.group.into());
        let servers = self.servers.iter().map(|s| s.clone().into()).collect();
        let instances = self
            .instances
            .0
            .values()
            .map(|i| i.clone().into())
            .collect();

        ClusterState {
            group,
            servers,
            instances,
        }
    }
}

impl TryFrom<ClusterState> for Cluster {
    type Error = Error;
    fn try_from(state: ClusterState) -> Result<Self> {
        let group = state
            .group
            .ok_or("Group cannot be empty".to_owned())?
            .try_into()?;

        let servers = state
            .servers
            .into_iter()
            .map(ServerInfo::try_from)
            .collect::<Result<_>>()?;

        let instances = state
            .instances
            .into_iter()
            .map(|i| AppInstancesInfo::try_from(i).map(|ii| (ii.deployment.id, ii)))
            .collect::<Result<HashMap<_, _, _>>>()
            .map(IdMap)?;

        let server_stats = IdMap::new();

        Ok(Self {
            group,
            servers,
            instances,
            server_stats,
        })
    }
}
