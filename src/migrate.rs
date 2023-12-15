use crate::{reader::read as wicked_read, MIGRATION_SETTINGS};
use agama_dbus_server::network::{model, Adapter, NetworkManagerAdapter, NetworkState};
use std::error::Error;

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
        let interfaces = wicked_read(self.paths.clone())?;
        let settings = MIGRATION_SETTINGS.get().unwrap();

        if !settings.continue_migration && interfaces.warning.is_some() {
            return Err(interfaces.warning.unwrap().into());
        }

        let mut state = NetworkState::new(vec![], vec![]);

        for interface in interfaces.interfaces {
            let connection_result = interface.to_connection()?;
            if !connection_result.warnings.is_empty() {
                for connection_error in &connection_result.warnings {
                    log::warn!("{}", connection_error);
                }
                if !settings.continue_migration {
                    return Err(anyhow::anyhow!(
                        "Migration of {} failed",
                        connection_result.connection.id()
                    )
                    .into());
                }
            }
            state.add_connection(connection_result.connection)?;
        }
        Ok(state)
    }

    fn write(&self, _network: &model::NetworkState) -> Result<(), Box<dyn std::error::Error>> {
        unimplemented!("not needed");
    }
}

pub async fn migrate(paths: Vec<String>) -> Result<(), Box<dyn Error>> {
    let wicked = WickedAdapter::new(paths);
    let state = wicked.read()?;
    let settings = MIGRATION_SETTINGS.get().unwrap();
    if settings.dry_run {
        for connection in state.connections {
            log::debug!("{:#?}", connection);
        }
        return Ok(());
    }
    let nm = NetworkManagerAdapter::from_system().await?;
    nm.write(&state)?;
    Ok(())
}
