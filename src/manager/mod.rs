mod reader;

use anyhow::{anyhow, Result};

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::process;
use std::process::Command;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    Mutex, RwLock,
};

use self::reader::read_all;

pub struct Service {
    pid: Mutex<u32>,
    opts: Arc<ServiceOpts>,
    status: Arc<Mutex<Status>>,
    indegree: u32,
    healthy_parents: RwLock<u32>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ServiceOpts {
    #[serde(skip)]
    name: String,
    cmd: String,
    health_check: String,
    #[serde(default)]
    combine_log: bool,
    #[serde(default)]
    one_shot: bool,
    #[serde(default)]
    after: Vec<String>,
}

enum Status {
    Pending,
    Init,
    Running,
    Successful,
    Stopped,
    Failure,
}

#[derive(Debug)]
enum ServiceSignal {
    Stop,
}

type ServiceName = String;

pub enum ManagerSignal {
    Healthy(ServiceName),
    AddService(ServiceOpts),
    DeleteService(ServiceName),
    StopService(ServiceName),
    StartService(ServiceName),
}

pub struct Manager<P: AsRef<Path>> {
    dir_parh: P,
    tracked_services: HashMap<String, Arc<Service>>,
    service_graph: HashMap<String, HashSet<String>>,
    service_senders: HashMap<String, Sender<ServiceSignal>>,
}

impl<P: AsRef<Path> + Send + Sync> Manager<P> {
    pub fn new(path: P) -> Self {
        Self {
            dir_parh: path,
            tracked_services: HashMap::new(),
            service_senders: HashMap::new(),
            service_graph: HashMap::new(),
        }
    }

    pub async fn monitor(
        &mut self,
        service_to_manager_sender: Sender<ManagerSignal>,
        service_to_manager_receiver: Receiver<ManagerSignal>,
    ) -> Result<()> {
        // loads services configurations from dir_path
        // generate new service objects for each option
        // starts a server listening for incoming requests
        let opts = read_all(self.dir_parh.as_ref())?;
        self.batch_add(opts)?;

        for (_, service) in self.tracked_services.clone() {
            // start whichever services that are ready to start
            self.start_if_eligible(service.clone(), service_to_manager_sender.clone())
                .await?;
        }

        self.listen(service_to_manager_sender, service_to_manager_receiver)
            .await;

        Ok(())
    }

    async fn is_eligible_to_start(&self, service: Arc<Service>) -> bool {
        let healthy_parents = service.healthy_parents.read().await;
        if service.indegree == *healthy_parents {
            return true;
        }

        service.change_status(Status::Pending).await;

        false
    }

    fn spawn_service(
        &mut self,
        service: Arc<Service>,
        service_to_manager_sender: Sender<ManagerSignal>,
    ) -> Result<()> {
        // create a channel
        // spawn a thread running the service, with the receiver end of the channel
        // store the sending end of the channel in the Manager's state

        let (manager_to_service_sender, manager_to_service_receiver) =
            mpsc::channel::<ServiceSignal>(10);

        // save sender to manager's map of service senders
        self.service_senders
            .insert(service.opts.name.clone(), manager_to_service_sender);

        tokio::spawn(async move {
            service
                .background_service(service_to_manager_sender, manager_to_service_receiver)
                .await
        });

        Ok(())
    }

    async fn increase_healthy_parents_of_children(
        &mut self,
        service: Arc<Service>,
        service_to_manager_sender: Sender<ManagerSignal>,
    ) -> Result<()> {
        // send increase healthy parents signal to all children of this service

        let mut children = Vec::<Arc<Service>>::new();
        if let Some(children_set) = self.service_graph.get(&service.opts.name) {
            for child_name in children_set {
                if let Some(child_service) = self.tracked_services.get(child_name) {
                    children.push(child_service.clone());
                }
            }
        }

        for child in children {
            child.increase_healthy_parents().await;
            self.start_if_eligible(child.clone(), service_to_manager_sender.clone())
                .await?;
        }

        Ok(())
    }

    async fn start_if_eligible(
        &mut self,
        service: Arc<Service>,
        service_to_manager_sender: Sender<ManagerSignal>,
    ) -> Result<()> {
        if self.is_eligible_to_start(service.clone()).await {
            self.spawn_service(service.clone(), service_to_manager_sender.clone())?;
        }

        Ok(())
    }

    async fn process_healthy_service(
        &mut self,
        service_name: &str,
        service_to_manager_sender: Sender<ManagerSignal>,
    ) {
        match self.tracked_services.get(service_name) {
            Some(service) => {
                if let Err(err) = self
                    .increase_healthy_parents_of_children(
                        service.clone(),
                        service_to_manager_sender.clone(),
                    )
                    .await
                {
                    log::error!("failed to process service {} signal: {}", service_name, err)
                }
            }
            None => {
                log::error!("no service with name {}", service_name);
            }
        }
    }

    async fn listen(
        &mut self,
        service_to_manager_sender: Sender<ManagerSignal>,
        mut service_to_manager_receiver: Receiver<ManagerSignal>,
    ) {
        loop {
            if let Some(sig) = service_to_manager_receiver.recv().await {
                match sig {
                    ManagerSignal::AddService(opts) => {
                        let service_name = opts.name.clone();
                        if let Err(err) = self.add(opts, service_to_manager_sender.clone()).await {
                            log::error!("failed to add service {}: {}", service_name, err);
                        }
                    }
                    ManagerSignal::DeleteService(service_name) => {
                        if let Err(err) = self.delete(&service_name).await {
                            log::error!("failed to delete service {}: {}", service_name, err);
                        }
                    }
                    ManagerSignal::Healthy(service_name) => {
                        self.process_healthy_service(
                            &service_name,
                            service_to_manager_sender.clone(),
                        )
                        .await;
                    }
                    ManagerSignal::StopService(service_name) => {
                        if let Err(err) = self.stop(&service_name).await {
                            log::error!("failed to stop service {}: {}", service_name, err);
                        }
                    }
                    ManagerSignal::StartService(service_name) => {
                        if let Err(err) = self
                            .start(&service_name, service_to_manager_sender.clone())
                            .await
                        {
                            log::error!("failed to start service {}: {}", service_name, err);
                        }
                    }
                }
                continue;
            }

            log::error!("manager channel with services was closed");
            return;
        }
    }

    async fn add(
        &mut self,
        opts: ServiceOpts,
        service_to_manager_sender: Sender<ManagerSignal>,
    ) -> Result<()> {
        // this method should add a service to tracked services and start this service if eligible
        self.validate_service_options(&opts)?;
        let service = Arc::new(new_service(Arc::new(opts)));
        self.tracked_services
            .insert(service.opts.name.clone(), service.clone());

        self.add_to_service_graph(&service.opts.name)?;

        self.start_if_eligible(service, service_to_manager_sender)
            .await?;

        Ok(())
    }

    fn validate_service_options(&self, opts: &ServiceOpts) -> Result<()> {
        // validate new service options before adding
        if self.tracked_services.get(&opts.name).is_some() {
            return Err(anyhow::anyhow!(
                "service name {} is already registered",
                opts.name
            ));
        }

        Ok(())
    }

    async fn delete(&mut self, service_name: &str) -> Result<()> {
        // this method should stop a service if running, and delete it from tracked services
        self.stop(service_name).await?;

        if let Some(service_sender) = self.service_senders.get(service_name) {
            service_sender.send(ServiceSignal::Stop).await?;
            if let Some(service) = self.tracked_services.remove(service_name) {
                self.remove_from_service_graph(service)?;
            }
            return Ok(());
        }

        Err(anyhow!("failed to find service with name {}", service_name))
    }

    fn remove_from_service_graph(&mut self, service: Arc<Service>) -> Result<()> {
        for parent in &service.opts.after {
            if let Some(set) = self.service_graph.get_mut(parent) {
                set.remove(&service.opts.name);
            }
        }

        Ok(())
    }

    async fn start(
        &mut self,
        service_name: &str,
        service_to_manager_sender: Sender<ManagerSignal>,
    ) -> Result<()> {
        // this method should start a tracked service.
        // this is done by spawning a separate thread for running this service until it gets a kill signal
        if let Some(service) = self.tracked_services.get(service_name) {
            self.start_if_eligible(service.clone(), service_to_manager_sender)
                .await?;

            return Ok(());
        }

        Err(anyhow!("failed to find service with name {}", service_name))
    }

    pub async fn stop(&self, service_name: &str) -> Result<()> {
        // this method should stop a tracked service.
        // this is done by sending a killsig to the service's process, and ending the service's thread
        if let Some(service) = self.tracked_services.get(service_name) {
            let pid = service.pid.lock().await;
            signal::kill(Pid::from_raw(*pid as i32), Signal::SIGKILL)?;
            if let Some(service_sender) = self.service_senders.get(service_name) {
                service_sender.send(ServiceSignal::Stop).await?;
            }

            service.change_status(Status::Stopped).await;
            return Ok(());
        }

        Err(anyhow!("failed to find service with name {}", service_name))
    }

    fn batch_add(&mut self, opts: HashMap<String, Arc<ServiceOpts>>) -> Result<()> {
        // this method should return the service map.
        // internally, it should generate the service graph, with each service knowing who are it's neighbours
        for (service_name, service_opts) in opts {
            self.tracked_services
                .insert(service_name.clone(), Arc::new(new_service(service_opts)));
        }

        for (_, service) in self.tracked_services.clone() {
            self.add_to_service_graph(&service.opts.name)?;
        }

        Ok(())
    }

    fn add_to_service_graph(&mut self, service_name: &str) -> Result<()> {
        if let Some(service) = self.tracked_services.get(service_name) {
            for parent in &service.opts.after {
                if self.tracked_services.get(parent).is_none() {
                    return Err(anyhow::anyhow!(
                        "failed to find service with name {}",
                        parent
                    ));
                }

                if !self.service_graph.contains_key(parent) {
                    self.service_graph
                        .insert(String::from(service_name), HashSet::new());
                }

                if let Some(set) = self.service_graph.get_mut(parent) {
                    set.insert(String::from(service_name));
                }
            }
        }

        return Err(anyhow::anyhow!(
            "no tracked service with name {}",
            service_name
        ));
    }
}

fn new_service(opts: Arc<ServiceOpts>) -> Service {
    Service {
        healthy_parents: RwLock::new(0),
        indegree: opts.after.len() as u32,
        opts,
        pid: Mutex::new(0),
        status: Arc::new(Mutex::new(Status::Init)),
    }
}

/*
   each service should be able to modify other services' state
   or, somehow tell the manager to modify other services' state
   solution:
       each service should have a channel with the manager, maybe the same channel that carries kill signals,
       this channel should tell the manager that service x is healthy, the manager then should notify all service dependents with this piece of info
*/

impl Service {
    async fn background_service(&self, tx: Sender<ManagerSignal>, mut rx: Receiver<ServiceSignal>) {
        /*
            each service needs to watch for:
                kill signals
                start signals
                signals from manager to increase or decrease healthy parents

            each of these signals need to be watched in a separate thread.

            kill singal procedure:
                - wait for the process to stop
                - terminate all spawned threads related to this service
                - terminate service thread
                > manager should first send a killsig to the service pid, then send the kill signal, then decrease number of healthy parents for dependent services.

            start signal procedure:
                - check if service is eligible to start
                - if true, start service, else change status to pending

            parents signal procedure:
                - if increase signal:
                    - increase number of healthy parents
                    - check if service is eligible to start, if true, start
                - if decrease signal, decrease number of healthy parents
        */

        loop {
            if rx.try_recv().is_err() {
                // this means that there is no incoming signal from the manager, service should keep running
                self.run(&tx).await;
                continue;
            }

            if let Ok(sig) = rx.try_recv() {
                match sig {
                    // ServiceSignal::Stop => self.stop_signal(),
                    // ServiceSignal::Start => self.start(&tx).await,
                    // ServiceSignal::IncreaseHealthyParents => self.increase_healthy_parents(),
                    // ServiceSignal::DecreaseHealthyParents => self.decrease_healthy_parents(),
                    ServiceSignal::Stop => {
                        // this block should handle the deletion of the service
                        return;
                    }
                };
            }
        }
    }

    async fn run(&self, tx: &Sender<ManagerSignal>) {
        let stdout_cfg = match self.opts.combine_log {
            false => process::Stdio::null(),
            true => process::Stdio::inherit(),
        };

        let mut ch = match Command::new("sh")
            .arg("-c")
            .args(self.opts.cmd.split(' '))
            .stdout(stdout_cfg)
            .spawn()
        {
            Err(err) => {
                log::error!("failed to spawn service {}: {}", self.opts.name, err);
                return;
            }
            Ok(ch) => ch,
        };

        self.change_pid(ch.id()).await;
        self.change_status(Status::Running).await;

        // send a message to the manager to indicate that this service is healthy
        if let Err(err) = tx
            .send(ManagerSignal::Healthy(self.opts.name.clone()))
            .await
        {
            log::error!(
                "failed to send healthy signal to service manager from service {}: {}",
                self.opts.name,
                err
            );
        }

        let exit_status = ch.wait();

        // process terminated
        self.change_pid(0).await;

        let code = match exit_status {
            Err(err) => {
                log::error!("failed to wait for command: {}", err);
                self.change_status(Status::Failure).await;
                return;
            }
            Ok(exit_status) => exit_status.code(),
        };

        if code != Some(0) {
            self.change_status(Status::Failure).await;
            return;
        }

        self.change_status(Status::Successful).await;
    }

    async fn change_pid(&self, new_pid: u32) {
        let mut pid = self.pid.lock().await;
        *pid = new_pid;
    }

    async fn change_status(&self, new_status: Status) {
        let mut status = self.status.lock().await;
        *status = new_status;
    }

    async fn increase_healthy_parents(&self) {
        let mut healthy_parents = self.healthy_parents.write().await;
        *healthy_parents += 1;
    }
}

/*
    service lifetime:
        - a service is a long living process.
        - if a service finished execution for any reason, it's respawned.
        - service could have one of a few statuses:
            - started
            - healthy (dependent service could start if eligible)
            - successful (service has terminated successfuly)
            - error (service has terminated due to some error)
            - stopped (service was terminated due to a kill signal from the manager)
            - pending (eligible to start but waiting for a start signal)
*/
