use crate::reader::read as wicked_read;
use agama_dbus_server::network::{model, Adapter, NetworkManagerAdapter, NetworkState};
use std::error::Error;
use tokio::{runtime::Handle, task};

struct WickedAdapter {
    paths: Vec<String>,
}

impl WickedAdapter {
    pub fn new(paths: Vec<String>) -> Self {
        Self { paths }
    }
}

impl Adapter for WickedAdapter {
    fn read(&self) -> Result<model::NetworkState, Box<dyn std::error::Error>> {
        task::block_in_place(|| {
            Handle::current().block_on(async {
                let interfaces = wicked_read(self.paths.clone())?;
                let mut state = NetworkState::new(vec![], vec![]);

                for interface in interfaces.interfaces {
                    let conn: model::Connection = interface.into();
                    state.add_connection(conn)?;
                }
                Ok(state)
            })
        })
    }

    fn write(&self, _network: &model::NetworkState) -> Result<(), Box<dyn std::error::Error>> {
        unimplemented!("not needed");
    }
}

pub async fn migrate(paths: Vec<String>) -> Result<(), Box<dyn Error>> {
    let wicked = WickedAdapter::new(paths);
    let state = wicked.read()?;
    let nm = NetworkManagerAdapter::from_system().await?;
    nm.write(&state)?;
    Ok(())
}
