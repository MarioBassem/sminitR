mod reader;
mod runner;
mod service;
mod tracker;

use anyhow::{anyhow, Error, Result};
use tokio::task::JoinHandle;

use std::{collections::HashMap, path::Path, sync::Arc};

use tokio::sync::mpsc::{self, Receiver, Sender};

use self::runner::Runner;
use self::service::{Service, ServiceOpts};
use self::tracker::Tracker;
use self::{reader::read_all, service::SimpleService};

type ServiceName = String;

#[derive(Debug)]
pub enum ManagerSignal {
    Healthy(ServiceName),
    AddService(ServiceOpts),
    DeleteService(ServiceName),
    StopService(ServiceName),
    StartService(ServiceName),
    CloseManager,
    ListServices,

    ServiceList(HashMap<String, Arc<Service>>),
}

#[derive(Debug)]
pub enum ManagerResponse {}

pub struct Manager<P: AsRef<Path>> {
    dir_parh: P,
    tracker: Arc<Tracker>,
    runner: Arc<Runner>,
    sender: Option<Sender<ManagerSignal>>,
}

async fn listen(
    tracker: Arc<Tracker>,
    runner: Arc<Runner>,
    tx: Sender<ManagerSignal>,
    mut rx: Receiver<ManagerSignal>,
) -> Result<()> {
    loop {
        println!("listenin*****************ig");
        // let sig2 = rx.try_recv()?;

        let sig = match rx.recv().await {
            Some(sig) => sig,
            None => {
                return Err(anyhow!("manager channel with services was closed"));
            }
        };
        // if runner.sender.is_closed() {
        //     info!("sender is closed****");
        // }

        // info!("received signal: {:?}", sig2);
        println!("in listen*************");
        match sig {
            ManagerSignal::Healthy(service_name) => {
                if let Err(err) = runner
                    .process_healthy_service(tracker.clone(), &service_name, tx.clone())
                    .await
                {
                    log::error!(
                        "failed to process healthy signal from service {}: {}",
                        service_name,
                        err
                    );
                }
            }
            _ => {}
        };

        // break;
        // if let Some(sig) = rx.recv().await {
        //     info!("received signal: {sig:?}");
        //     match sig {
        //         // ManagerSignal::AddService(opts) => {
        //         //     let service_name = opts.name.clone();
        //         //     if let Err(err) = self.add(opts, service_to_manager_sender.clone()).await {
        //         //         log::error!("failed to add service {}: {}", service_name, err);
        //         //     }
        //         // }
        //         // ManagerSignal::DeleteService(service_name) => {
        //         //     if let Err(err) = self.delete(&service_name).await {
        //         //         log::error!("failed to delete service {}: {}", service_name, err);
        //         //     }
        //         // }
        //         ManagerSignal::Healthy(service_name) => {
        //             if let Err(err) = runner
        //                 .process_healthy_service(tracker.clone(), &service_name)
        //                 .await
        //             {
        //                 log::error!(
        //                     "failed to process healthy signal from service {}: {}",
        //                     service_name,
        //                     err
        //                 );
        //             }
        //         }
        //         // ManagerSignal::StopService(service_name) => {
        //         //     if let Err(err) = self.stop(&service_name).await {
        //         //         log::error!("failed to stop service {}: {}", service_name, err);
        //         //     }
        //         // }
        //         // ManagerSignal::StartService(service_name) => {
        //         //     if let Err(err) = self
        //         //         .start(&service_name, service_to_manager_sender.clone())
        //         //         .await
        //         //     {
        //         //         log::error!("failed to start service {}: {}", service_name, err);
        //         //     }
        //         // }
        //         // ManagerSignal::CloseManager => {
        //         //     service_to_manager_receiver.close();
        //         //     return Ok(());
        //         // }
        //         // ManagerSignal::ListServices => {
        //         //     if let Err(err) = manager_to_server_sender
        //         //         .send(ManagerSignal::ServiceList(self.tracked_services.clone()))
        //         //         .await
        //         //     {
        //         //         log::error!("failed to lsit services: {}", err);
        //         //     }
        //         // }
        //         _ => {}
        //     }
        //     continue;
        // }
        // error!("exiting listen*****");
    }
    Ok(())
}

impl<P: AsRef<Path> + Send + Sync> Manager<P> {
    pub async fn new(path: P) -> Self {
        // let (tx, rx) = mpsc::channel(100);
        let tracker = Arc::new(Tracker::new());
        let runner = Arc::new(Runner {});
        // let tracker_clone = tracker.clone();
        // let runner_clone = runner.clone();

        // tokio::spawn(async move { listen(tracker_clone, runner_clone, rx).await });

        Self {
            dir_parh: path,
            tracker,
            runner,
            sender: None,
        }
    }

    pub async fn monitor(&self) -> Result<()> {
        let (tx, rx) = mpsc::channel(100);
        let opts = read_all(self.dir_parh.as_ref())?;
        self.runner
            .batch_add(self.tracker.clone(), opts, tx.clone())
            .await?;

        listen(self.tracker.clone(), self.runner.clone(), tx, rx).await?;

        // let handle = tokio::spawn(listen(self.tracker.clone(), self.runner.clone(), tx, rx));

        Ok(())
    }

    pub async fn add(&self, service_name: &str) -> Result<()> {
        let opts = reader::try_read_service(service_name, self.dir_parh.as_ref())?;
        let sender = match &self.sender {
            Some(sender) => sender,
            None => return Err(anyhow!("run monitor first")),
        };
        self.runner
            .add(opts, self.tracker.clone(), sender.clone())
            .await
    }

    pub async fn delete(&self, service_name: &str) -> Result<()> {
        self.runner.delete(service_name, self.tracker.clone()).await
    }

    pub async fn stop(&self, service_name: &str) -> Result<()> {
        self.runner.stop(service_name, self.tracker.clone()).await
    }

    pub async fn start(&self, service_name: &str) -> Result<()> {
        let sender = match &self.sender {
            Some(sender) => sender,
            None => return Err(anyhow!("run monitor first")),
        };
        self.runner
            .start(service_name, self.tracker.clone(), sender.clone())
            .await
    }

    pub async fn list(&self) -> Vec<SimpleService> {
        self.tracker.list_services().await
    }

    // pub async fn monitor(&mut self) -> Result<(Sender<ManagerSignal>, Receiver<ManagerSignal>)> {
    //     // loads services configurations from dir_path
    //     // generate new service objects for each option
    //     // starts a server listening for incoming requests
    //     log::info!("monitoring(((");
    //     let opts: HashMap<String, Arc<ServiceOpts>> = read_all(self.dir_parh.as_ref())?;
    //     self.batch_add(opts)?;

    //     let (to_manager_sender, to_manager_receiver) = mpsc::channel(100);
    //     let (manager_to_server_sender, manager_to_server_receiver) = mpsc::channel(100);

    //     for (_, service) in self.tracked_services.clone() {
    //         // start whichever services that are ready to start
    //         self.start_if_eligible(service.clone(), to_manager_sender.clone())
    //             .await?;
    //     }

    //     self.listen(
    //         to_manager_sender.clone(),
    //         to_manager_receiver,
    //         manager_to_server_sender,
    //     )
    //     .await?;

    //     Ok((to_manager_sender, manager_to_server_receiver))
    // }

    // async fn increase_healthy_parents_of_children(
    //     &mut self,
    //     service: Arc<Service>,
    //     service_to_manager_sender: Sender<ManagerSignal>,
    // ) -> Result<()> {
    //     // send increase healthy parents signal to all children of this service

    //     let mut children = Vec::<Arc<Service>>::new();
    //     if let Some(children_set) = self.service_graph.get(&service.opts.name) {
    //         for child_name in children_set {
    //             if let Some(child_service) = self.tracked_services.get(child_name) {
    //                 children.push(child_service.clone());
    //             }
    //         }
    //     }

    //     for child in children {
    //         child.increase_healthy_parents().await;
    //         self.start_if_eligible(child.clone(), service_to_manager_sender.clone())
    //             .await?;
    //     }

    //     Ok(())
    // }

    // async fn start_if_eligible(
    //     &mut self,
    //     service: Arc<Service>,
    //     service_to_manager_sender: Sender<ManagerSignal>,
    // ) -> Result<()> {
    //     if self.is_eligible_to_start(service.clone()).await {
    //         self.spawn_service(service.clone(), service_to_manager_sender.clone())
    //             .await?;
    //     }

    //     Ok(())
    // }

    // async fn process_healthy_service(
    //     &mut self,
    //     service_name: &str,
    //     service_to_manager_sender: Sender<ManagerSignal>,
    // ) {
    //     match self.tracked_services.get(service_name) {
    //         Some(service) => {
    //             if let Err(err) = self
    //                 .increase_healthy_parents_of_children(
    //                     service.clone(),
    //                     service_to_manager_sender.clone(),
    //                 )
    //                 .await
    //             {
    //                 log::error!("failed to process service {} signal: {}", service_name, err)
    //             }
    //         }
    //         None => {
    //             log::error!("no service with name {}", service_name);
    //         }
    //     }
    // }

    // async fn listen(
    //     &mut self,
    //     service_to_manager_sender: Sender<ManagerSignal>,
    //     mut service_to_manager_receiver: Receiver<ManagerSignal>,
    //     manager_to_server_sender: Sender<ManagerSignal>,
    // ) -> Result<()> {
    //     loop {
    //         // println!("iteration***");
    //         if let Some(sig) = service_to_manager_receiver.recv().await {
    //             println!("received signal: {:?}", sig);
    //             match sig {
    //                 ManagerSignal::AddService(opts) => {
    //                     let service_name = opts.name.clone();
    //                     if let Err(err) = self.add(opts, service_to_manager_sender.clone()).await {
    //                         log::error!("failed to add service {}: {}", service_name, err);
    //                     }
    //                 }
    //                 ManagerSignal::DeleteService(service_name) => {
    //                     if let Err(err) = self.delete(&service_name).await {
    //                         log::error!("failed to delete service {}: {}", service_name, err);
    //                     }
    //                 }
    //                 ManagerSignal::Healthy(service_name) => {
    //                     self.process_healthy_service(
    //                         &service_name,
    //                         service_to_manager_sender.clone(),
    //                     )
    //                     .await;
    //                 }
    //                 ManagerSignal::StopService(service_name) => {
    //                     if let Err(err) = self.stop(&service_name).await {
    //                         log::error!("failed to stop service {}: {}", service_name, err);
    //                     }
    //                 }
    //                 ManagerSignal::StartService(service_name) => {
    //                     if let Err(err) = self
    //                         .start(&service_name, service_to_manager_sender.clone())
    //                         .await
    //                     {
    //                         log::error!("failed to start service {}: {}", service_name, err);
    //                     }
    //                 }
    //                 ManagerSignal::CloseManager => {
    //                     service_to_manager_receiver.close();
    //                     return Ok(());
    //                 }
    //                 ManagerSignal::ListServices => {
    //                     if let Err(err) = manager_to_server_sender
    //                         .send(ManagerSignal::ServiceList(self.tracked_services.clone()))
    //                         .await
    //                     {
    //                         log::error!("failed to lsit services: {}", err);
    //                     }
    //                 }
    //                 _ => {}
    //             }
    //             continue;
    //         }

    //         return Err(anyhow!("manager channel with services was closed"));
    //     }
    // }

    // async fn add(
    //     &mut self,
    //     opts: ServiceOpts,
    //     service_to_manager_sender: Sender<ManagerSignal>,
    // ) -> Result<()> {
    //     // this method should add a service to tracked services and start this service if eligible
    //     self.validate_service_options(&opts)?;
    //     let service = Arc::new(new_service(Arc::new(opts)));
    //     self.tracked_services
    //         .insert(service.opts.name.clone(), service.clone());

    //     self.add_to_service_graph(&service.opts.name)?;

    //     self.start_if_eligible(service, service_to_manager_sender)
    //         .await?;

    //     Ok(())
    // }

    // fn validate_service_options(&self, opts: &ServiceOpts) -> Result<()> {
    //     // validate new service options before adding
    //     if self.tracked_services.get(&opts.name).is_some() {
    //         return Err(anyhow::anyhow!(
    //             "service name {} is already registered",
    //             opts.name
    //         ));
    //     }

    //     Ok(())
    // }

    // async fn delete(&mut self, service_name: &str) -> Result<()> {
    //     // this method should stop a service if running, and delete it from tracked services
    //     self.stop(service_name).await?;

    //     if let Some(service_sender) = self.service_senders.get(service_name) {
    //         service_sender.send(ServiceSignal::Stop).await?;
    //         if let Some(service) = self.tracked_services.remove(service_name) {
    //             self.remove_from_service_graph(service)?;
    //         }
    //         return Ok(());
    //     }

    //     Err(anyhow!("failed to find service with name {}", service_name))
    // }

    // fn remove_from_service_graph(&mut self, service: Arc<Service>) -> Result<()> {
    //     for parent in &service.opts.after {
    //         if let Some(set) = self.service_graph.get_mut(parent) {
    //             set.remove(&service.opts.name);
    //         }
    //     }

    //     Ok(())
    // }

    // async fn start(
    //     &mut self,
    //     service_name: &str,
    //     service_to_manager_sender: Sender<ManagerSignal>,
    // ) -> Result<()> {
    //     // this method should start a tracked service.
    //     // this is done by spawning a separate thread for running this service until it gets a kill signal
    //     if let Some(service) = self.tracked_services.get(service_name) {
    //         self.start_if_eligible(service.clone(), service_to_manager_sender)
    //             .await?;

    //         return Ok(());
    //     }

    //     Err(anyhow!("failed to find service with name {}", service_name))
    // }
}

#[cfg(test)]
mod test {
    use std::{fs::File, io::Write};

    use tempfile::TempDir;

    use super::Manager;
    use std::{thread, time};

    #[tokio::test(flavor = "multi_thread", worker_threads = 10)]
    async fn test_manager() {
        let dir = TempDir::new().unwrap();
        let dir_path = dir.path();
        let file_name = String::from("hello.yaml");
        let mut tmpfile = File::create(dir_path.join(file_name)).unwrap();
        // tmpfile
        //     .write_all(
        //         b"\ncmd: echo \"hi world\"\nhealth_check: sleep 1\ncombine_log: true\none_shot: false\n",
        //     )
        //     .unwrap();
        // let manager = Manager::init(dir_path);

        // manager.monitor().await.unwrap();
        // thread::sleep(time::Duration::from_secs(3));
        // manager.stop("hello").await.unwrap();

        // tmpfile = File::create(dir_path.join(String::from("world.yaml"))).unwrap();
        // tmpfile.write_all(b"\ncmd: echo \"back at you\"\nhealth_check: sleep 1\ncombine_log: true\none_shot: false\nafter: \n - hello\n",).unwrap();
        // // manager.add("world").await;

        // thread::sleep(time::Duration::from_secs(3));
        // manager.add("world").await;
        // let res = manager.list().await;
        // println!("res: {:?}", res);
        // manager.start("hello").await.unwrap();
        thread::sleep(time::Duration::from_secs(1));
        // manager.add(service_name)
    }
}
