use crate::{reader::read as wicked_read, MIGRATION_SETTINGS};
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

                if !MIGRATION_SETTINGS.get().unwrap().continue_migration
                    && interfaces.error.is_some()
                {
                    Err(interfaces.error.unwrap())?
                }

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
