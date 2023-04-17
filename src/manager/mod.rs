mod reader;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc, path::Path, borrow::{BorrowMut, Borrow},
};
use tokio::{
    sync::{mpsc::{Sender, self, Receiver}, Mutex, RwLock},
};
use std::process::Command;

use self::reader::read_all;

pub struct Service {
    pid : Mutex<u32>,
    opts: Arc<ServiceOpts>,
    status: Arc<Mutex<Status>>,
    indegree: u32,
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
    Running,
}

enum ServiceSignal{
    Stop,
    Delete,
    Start,
    IncreaseHealthyParents,
    DecreaseHealthyParents,
}

enum ManagerSignal{
    Healthy,
}

struct Senders{
    start: Sender<bool>,
    kill: Sender<ServiceSignal>,
}

pub struct Manager<P: AsRef<Path>> {
    dir_parh: P,
    tracked_services: Arc<HashMap<String, Arc<Service>>>,
    senders: Arc<HashMap<String, Sender<ServiceSignal>>>,
}   

impl<P: AsRef<Path>> Manager<P> {
    pub fn new(path: P) -> Self {
        Self { dir_parh: path, tracked_services: Arc::new(HashMap::new()), senders: Arc::new(HashMap::new()) }
    }

    pub fn init(&mut self) -> Result<()> {
        // loads services configurations from dir_path
        // generate new service objects for each option
        // starts a server listening for incoming requests
        let opts = read_all(self.dir_parh.as_ref())?;
        let services = self.generate_services(opts)?;

        self.tracked_services = Arc::new(services);
        
        for (_, service) in self.tracked_services.clone().iter(){
            self.spawn_service(service.clone())?;
            if self.is_eligible_to_start(service.clone()){
                // this service is eligible to start, manager should spawn it in a thread
                self.start_service(service.clone())?;
            }
        }

        return Ok(())
    }

    fn spawn_service(&mut self, service: Arc<Service>) -> Result<()>{
        // create a channel
        // spawn a thread running the service, with the receiver end of the channel
        // store the sending end of the channel in the Manager's state
        let (tx, rx) = mpsc::channel::<ServiceSignal>(10);
        let (tx2, mut rx2) = mpsc::channel::<bool>(10);
        let service_clone = service.clone();
        let opts = service.opts.clone();
        let senders = self.senders.clone();
        // tokio::spawn(async move { service.background_service(&tx2, rx).await; });
        tokio::spawn( async move {service_clone.background_service(&tx2, rx).await});
        tokio::spawn(async move{
            match rx2.recv().await{
                Some(_) => {
                    // increase dependents healthy parents
                    for var in opts.after.iter(){
                        let x = senders.get(var);
                        x.unwrap().send(ServiceSignal::IncreaseHealthyParents);
                    }
                    todo!()
                }
                None => {
                    return
                }
            }
        });
        
        Ok(())
    }

    fn bulk_add(&mut self, service_options: HashMap<String, ServiceOpts>) -> Result<()>{
        // this method should add multiple service options together. 
        // it is only called whenever a new instance of the manager is created with multiple services
        for (_, opt) in service_options{
            
        }
        todo!()
    }

    pub fn add(&mut self, service: Arc<Service>) -> Result<()> {
        // this method should add a service to tracked services and start this service if eligible
        todo!()
    }

    pub fn delete(&mut self, service: Arc<Service>) -> Result<()> {
        // this method should stop a service if running, and delete it from tracked services 
        todo!()
    }

    pub fn start_service(&self, service: Arc<Service>) -> Result<()> {
        // this method should start a tracked service.
        // this is done by spawning a separate thread for running this service until it gets a kill signal
        todo!()
    }

    pub fn stop(&self, service: Arc<Service>) -> Result<()> {
        // this method should stop a tracked service.
        // this is done by sending a killsig to the service's process, and ending the service's thread
        todo!()
    }

    pub fn list(&self) -> Result<Vec<Service>> {
        // this method should list all tracked services
        todo!()
    }

    fn generate_services(&self, opts: HashMap<String, ServiceOpts>) -> Result<HashMap<String, Arc<Service>>>{
        // this method should return the service map.
        // internally, it should generate the service graph, with each service knowing who are it's neighbours
        todo!()
    }

    fn is_eligible_to_start(&self, service: Arc<Service>) -> bool{
        // this method should return true if the provided service has all needed dependencies
        todo!()
    }
}

/*
    each service should be able to modify other services' state
    or, somehow tell the manager to modify other services' state
    solution:
        each service should have a channel with the manager, maybe the same channel that carries kill signals, 
        this channel should tell the manager that service x is healthy, the manager then should notify all service dependents with this piece of info 
 */

impl Service{
    async fn background_service(&self, tx: &Sender<bool>,mut rx: Receiver<ServiceSignal>){
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
        // tokio::spawn(async {self.signal_watcher().await;});
        loop{
            if let Some(sig) = rx.recv().await{
                match sig{
                    ServiceSignal::Stop => self.stop_signal(),
                    ServiceSignal::Start => self.start_signal(tx),
                    ServiceSignal::IncreaseHealthyParents => self.increase_healthy_parents(),
                    ServiceSignal::DecreaseHealthyParents => self.decrease_healthy_parents(),
                    ServiceSignal::Delete => {
                        // this block should handle the deletion of the service
                        return;
                    },
                };
            }
            
        }

    }

    fn stop_signal(&self){
        // this method should handle how the service gets stopped
        // - send killsig to the process pid
        // - change service status
        todo!()
    }

    fn increase_healthy_parents(&self) {
        todo!()
    }

    fn decrease_healthy_parents(&self) {
        todo!()
    }

    fn start_signal(&self, tx: &Sender<bool>){
        // this procedure handles how the service receives a start signal
        // service should check if eligible to start
        // if eligible, spawn a thread to run the process
        // if not, change status to pending
        todo!()
    }

}