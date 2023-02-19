mod reader;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc, path::Path, borrow::BorrowMut,
};
use tokio::{
    sync::{mpsc, watch::{Sender, self, Receiver}, Mutex},
};

use self::reader::read_all;

pub struct Service {
    opts: ServiceOpts,
    status: Arc<Mutex<Status>>,
    neighbours: Mutex<HashSet<Neighbours>>,
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

struct Neighbours {
    children: HashSet<String>,
    parents: HashSet<String>,
}

enum Status {
    Running,
}

enum KillSig{
    Stop,
    Delete,
}

struct Senders{
    start: Sender<bool>,
    kill: Sender<KillSig>,
}

pub struct Manager<P: AsRef<Path>> {
    dir_parh: P,
    tracked_services: HashMap<String, Arc<Service>>,
    senders: HashMap<String, Senders>,
}

impl<P: AsRef<Path>> Manager<P> {
    pub fn new(path: P) -> Self {
        Self { dir_parh: path, tracked_services: HashMap::new(), senders: HashMap::new() }
    }

    pub fn init(&mut self) -> Result<()> {
        // loads services configurations from dir_path
        // generate new service objects for each option
        // for each service, spawns a separate thread
        // starts a server listening for incoming requests
        let opts = read_all(self.dir_parh.as_ref())?;
        let services = self.generate_services(opts)?;

        self.tracked_services = services;
        for (k, service) in &self.tracked_services{
            let (start_sender, start_receiver) = watch::channel(true);
            let (kill_sender, kill_receiver) = watch::channel(KillSig::Stop);
            self.senders.insert(k.to_string(), Senders { start: start_sender, kill: kill_sender });
            // spawn a thread that takes service and tx
            tokio::spawn(background_service(service.clone(), start_receiver, kill_receiver));
        }

        todo!()
    }

    pub fn add() -> Result<()> {
        todo!()
    }

    pub fn delete() -> Result<()> {
        todo!()
    }

    pub fn start() -> Result<()> {
        todo!()
    }

    pub fn stop() -> Result<()> {
        todo!()
    }

    pub fn list() -> Result<Vec<Service>> {
        todo!()
    }

    fn generate_services(&self, opts: HashMap<String, ServiceOpts>) -> Result<HashMap<String, Arc<Service>>>{
        todo!()
    }
}

async fn background_service(service: Arc<Service>, mut start_receiver: Receiver<bool>, mut kill_receiver: Receiver<KillSig>){
    // this should do two operations concurrently:
    // 1- wait for a signal on tx
    // 2- run service if a start signal is received
    // 3- if a stop signal is received, service should stop
    // 4- if a delete signal is received, service should stop, and this function returns
    loop{
        if let Err(err) = start_receiver.changed().await{
            println!("sminit encountered receive error: {}", err);
            continue;
        }
        // check if service is eligible to start
        // spawn a thread to run the service
        if !is_eligible_to_start(service.clone()){
            continue;
        }
        run_service(service.clone(), &mut kill_receiver).await;
    };
}

fn is_eligible_to_start(service: Arc<Service>) -> bool{
    todo!()
}

async fn run_service(service: Arc<Service>, kill_receiver: &mut Receiver<KillSig>){
    loop{
        if let Err(err) = kill_receiver.changed().await{
            println!("sminit encountered receive error: {}", err);
            continue;
        }
    }
}