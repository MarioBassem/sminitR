use std::path::Path;

use manager::Manager;

pub mod manager;
use env_logger::Builder;
use log::LevelFilter;
use std::env;
use std::io::Write;
use std::{thread, time};

#[macro_use]
extern crate log;

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
async fn main() {
    // log::info!()
    // info!("running*******");
    Builder::new()
        .default_format()
        .filter_level(LevelFilter::Debug)
        .init();

    let path = Path::new("/home/mariocs/cs/sminitr/test");
    let manager = Manager::new(path).await;
    manager.monitor().await.unwrap();
    thread::sleep(time::Duration::from_secs(3));
    // if handler.is_finished() {
    //     info!("handler is finished")
    // }

    thread::sleep(time::Duration::from_secs(3));
    let list = manager.list().await;
    info!("list: {:?}", list);
}
