use anyhow::{anyhow, Result};

use tokio::sync::{mpsc::Sender, Mutex, RwLock};

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::manager::service;

use super::service::{Service, ServiceOpts, ServiceSignal, SimpleService, Status};

pub struct Tracker {
    tracked: RwLock<HashMap<u64, Arc<Service>>>,
    graph: RwLock<HashMap<u64, HashSet<u64>>>,
    senders: RwLock<HashMap<u64, Sender<ServiceSignal>>>,
    id_map: RwLock<HashMap<String, u64>>,
    id: Mutex<u64>,
}

impl Tracker {
    pub fn new() -> Self {
        Self {
            tracked: RwLock::new(HashMap::new()),
            graph: RwLock::new(HashMap::new()),
            senders: RwLock::new(HashMap::new()),
            id: Mutex::new(1),
            id_map: RwLock::new(HashMap::new()),
        }
    }

    pub async fn add(&self, opts: ServiceOpts) -> Result<()> {
        // add to memory (tracked and graph only)

        let mut id_map_guard = self.id_map.write().await;
        if id_map_guard.contains_key(&opts.name) {
            return Err(anyhow!(
                "service with name {} is already registered",
                opts.name
            ));
        }

        let mut parent_set = HashSet::new();
        for parent in &opts.after {
            if let Some(id) = id_map_guard.get(parent) {
                parent_set.insert(*id);
            } else {
                return Err(anyhow!("service {} is not registered", parent));
            }
        }

        let service = Arc::new(self.new_service(opts).await);

        id_map_guard.insert(service.opts.name.clone(), service.id);

        let mut tracked_guard = self.tracked.write().await;
        tracked_guard.insert(service.id, service.clone());

        let mut graph_guard = self.graph.write().await;
        graph_guard.insert(service.id, parent_set);

        Ok(())
    }

    pub async fn batch_add(&self, opts_map: HashMap<String, ServiceOpts>) -> Result<()> {
        // adds all services in dir_path
        // ** this is only called when sminit starts

        let mut id_map_guard = self.id_map.write().await;
        for (_, opts) in &opts_map {
            // we are sure that all services are unique
            // but we need to validate the "after" field in each service

            for parent in &opts.after {
                if !opts_map.contains_key(parent) {
                    return Err(anyhow!("service with name {} is not registered", parent));
                }
            }
        }

        let mut tracked_guard = self.tracked.write().await;
        let mut graph_guard = self.graph.write().await;
        for (_, opts) in &opts_map {
            let service = Arc::new(self.new_service(opts.clone()).await);
            id_map_guard.insert(service.opts.name.clone(), service.id);
            tracked_guard.insert(service.id, service.clone());
            graph_guard.insert(service.id, HashSet::new());
        }

        for (service_name, opts) in &opts_map {
            // this has to be done in a separate loop
            // let mut set = HashSet::new();
            // let service_id = match id_map_guard.get(service_name) {
            //     Some(id) => id,
            //     None => return Err(anyhow!("failed to find service {} id", service_name)),
            // };
            if let Some(service_id) = id_map_guard.get(service_name) {
                for parent in &opts.after {
                    if let Some(parent_id) = id_map_guard.get(parent) {
                        // assuming its always present
                        if let Some(set) = graph_guard.get_mut(parent_id) {
                            set.insert(*service_id);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn remove(&self, service_name: &str) -> Result<()> {
        // remove from memory (all)
        let mut id_map_guard = self.id_map.write().await;
        let service_id = match id_map_guard.get(service_name) {
            Some(id) => id,
            None => return Err(anyhow!("service {} is not registered", service_name)),
        };

        let mut tracked_guard = self.tracked.write().await;
        tracked_guard.remove(service_id);

        let mut graph_guard = self.graph.write().await;
        graph_guard.remove(service_id);

        let mut senders_guard = self.senders.write().await;
        senders_guard.remove(service_id);

        id_map_guard.remove(service_name);

        Ok(())
    }

    pub async fn get_service(&self, service_name: &str) -> Result<Arc<Service>> {
        let id_map_guard = self.id_map.read().await;
        let service_id = match id_map_guard.get(service_name) {
            Some(id) => id,
            None => return Err(anyhow!("service {} is not registered", service_name)),
        };

        let tracked_guard = self.tracked.read().await;

        let service = match tracked_guard.get(service_id) {
            Some(service) => service.clone(),
            None => {
                return Err(anyhow!(
                    "service {} is not found in tracked services",
                    service_name
                ))
            }
        };

        Ok(service)
    }

    pub async fn get_children(&self, service_name: &str) -> Result<Vec<Arc<Service>>> {
        let id_map_guard = self.id_map.read().await;
        let mut children = Vec::new();
        let service_id = match id_map_guard.get(service_name) {
            Some(id) => id,
            None => return Err(anyhow!("service {} is not registered", service_name)),
        };

        let graph_guard = self.graph.read().await;
        let service_children_set = match graph_guard.get(service_id) {
            Some(set) => set,
            None => {
                return Err(anyhow!(
                    "service {} could not be found in graph",
                    service_name
                ))
            }
        };

        let tracked_guard = self.tracked.read().await;
        for k in service_children_set {
            let service = match tracked_guard.get(k) {
                Some(service) => service,
                None => {
                    return Err(anyhow!(
                        "service with id {} could not be found in tracked services",
                        k
                    ))
                }
            };
            children.push(service.clone());
        }

        Ok(children)
    }

    pub async fn get_sender(&self, service_name: &str) -> Result<Sender<ServiceSignal>> {
        let id_map_guard = self.id_map.read().await;
        let senders_guard = self.senders.read().await;

        let service_id = match id_map_guard.get(service_name) {
            Some(id) => id,
            None => return Err(anyhow!("service {} is not registered", service_name)),
        };

        if let Some(sender) = senders_guard.get(service_id) {
            return Ok(sender.clone());
        }

        return Err(anyhow!(
            "failed to find sender for service {}",
            service_name
        ));
    }

    pub async fn new_service(&self, opts: ServiceOpts) -> Service {
        let mut id = self.id.lock().await;
        let service = Service {
            id: *id,
            healthy_parents: RwLock::new(0),
            indegree: opts.after.len() as u32,
            opts,
            pid: Mutex::new(0),
            status: Arc::new(Mutex::new(Status::Init)),
        };

        *id += 1;

        service
    }

    pub async fn insert_sender(
        &self,
        service_name: &str,
        sender: Sender<ServiceSignal>,
    ) -> Result<()> {
        let id_map_guard = self.id_map.read().await;
        let mut senders_guard = self.senders.write().await;

        if let Some(service_id) = id_map_guard.get(service_name) {
            senders_guard.insert(*service_id, sender);
        } else {
            return Err(anyhow!("service {} is not registered", service_name));
        }

        Ok(())
    }

    pub async fn remove_sender(&self, service_name: &str) -> Result<()> {
        let id_map_guard = self.id_map.read().await;
        let mut senders_guard = self.senders.write().await;

        if let Some(service_id) = id_map_guard.get(service_name) {
            senders_guard.remove(service_id);
        } else {
            return Err(anyhow!("service {} is not registered", service_name));
        }

        Ok(())
    }

    pub async fn list_services(&self) -> Vec<SimpleService> {
        let mut list = Vec::new();
        let tracked_guard = self.tracked.read().await;
        for (_, v) in tracked_guard.iter() {
            list.push(v.simplify().await);
        }

        list
    }
}
