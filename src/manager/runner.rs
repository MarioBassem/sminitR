use std::{collections::HashMap, sync::Arc};

use tokio::sync::mpsc::{self, Receiver, Sender};

use super::{
    service::{Service, ServiceOpts, ServiceSignal, Status},
    tracker::Tracker,
    ManagerSignal,
};

use anyhow::{anyhow, Result};

pub struct Runner {}

impl Runner {
    pub async fn batch_add(
        &self,
        tracker: Arc<Tracker>,
        opts: HashMap<String, ServiceOpts>,
        tx: Sender<ManagerSignal>,
    ) -> Result<()> {
        let mut service_names = Vec::new();
        for (name, _) in opts.clone() {
            service_names.push(name)
        }
        tracker.batch_add(opts).await?;
        for service_name in service_names {
            if let Err(err) = self.start(&service_name, tracker.clone(), tx.clone()).await {
                log::info!("failed to start service: {}", err);
            }
        }
        Ok(())
    }
    pub async fn add(
        &self,
        opts: ServiceOpts,
        tracker: Arc<Tracker>,
        tx: Sender<ManagerSignal>,
    ) -> Result<()> {
        // generate service struct, add to tracker, spawn
        let service_name = opts.name.clone();
        tracker.add(opts).await?;
        if let Err(err) = self.start(&service_name, tracker, tx).await {
            log::info!("failed to start service: {}", err);
        }
        Ok(())
    }

    pub async fn delete(&self, service_name: &str, tracker: Arc<Tracker>) -> Result<()> {
        // stop service, delete from tracker
        self.stop(service_name, tracker.clone()).await?;
        tracker.remove(service_name).await?;
        Ok(())
    }

    pub async fn stop(&self, service_name: &str, tracker: Arc<Tracker>) -> Result<()> {
        // stop service
        let service_sender = tracker.get_sender(service_name).await?;
        service_sender.send(ServiceSignal::Stop).await?;
        tracker.remove_sender(service_name).await?;
        Ok(())
    }

    pub async fn start(
        &self,
        service_name: &str,
        tracker: Arc<Tracker>,
        manager_sender: Sender<ManagerSignal>,
    ) -> Result<()> {
        // spawn service
        let service = tracker.get_service(service_name).await?;
        if !self.is_eligible_to_start(service.clone()).await {
            return Err(anyhow!("service {} is still pending", service_name));
        }

        let (tx, rx) = mpsc::channel::<ServiceSignal>(1);

        tracker.insert_sender(service_name, tx).await?;

        tokio::spawn(async move { service.background_service(manager_sender, rx).await });

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

    pub async fn process_healthy_service(
        &self,
        tracker: Arc<Tracker>,
        service_name: &str,
        tx: Sender<ManagerSignal>,
    ) -> Result<()> {
        tracker.get_service(service_name).await?;
        self.increase_healthy_parents_of_children(tracker, service_name, tx)
            .await?;
        Ok(())
    }

    async fn increase_healthy_parents_of_children(
        &self,
        tracker: Arc<Tracker>,
        service_name: &str,
        tx: Sender<ManagerSignal>,
    ) -> Result<()> {
        // send increase healthy parents signal to all children of this service
        let children = tracker.get_children(service_name).await?;
        info!("children: {:?}", children);
        for child in children {
            child.increase_healthy_parents().await;
            if let Err(err) = self.start(service_name, tracker.clone(), tx.clone()).await {
                log::info!("failed to start service: {}", err);
            }
        }

        Ok(())
    }
}
