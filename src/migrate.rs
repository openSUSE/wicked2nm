use crate::{reader::read as wicked_read, MIGRATION_SETTINGS};
use agama_dbus_server::network::{model, Adapter, NetworkManagerAdapter, NetworkState};
use async_trait::async_trait;
use std::{collections::HashMap, error::Error};
use uuid::Uuid;

struct WickedAdapter {
    paths: Vec<String>,
}

impl WickedAdapter {
    pub fn new(paths: Vec<String>) -> Self {
        Self { paths }
    }
}

fn update_parent_connection(
    state: &mut model::NetworkState,
    parents: HashMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let settings = MIGRATION_SETTINGS.get().unwrap();
    let mut parent_uuid: HashMap<String, Uuid> = HashMap::new();

    for (id, parent) in parents {
        if let Some(parent_con) = state.get_connection_by_interface(&parent) {
            parent_uuid.insert(id, parent_con.uuid);
        } else {
            log::warn!("Missing parent {} connection for {}", parent, id);
            if !settings.continue_migration {
                return Err(anyhow::anyhow!("Migration of {} failed", id).into());
            }
        }
    }

    for (id, uuid) in parent_uuid {
        if let Some(connection) = state.get_connection_mut(&id) {
            connection.controller = Some(uuid);
        } else {
            return Err(anyhow::anyhow!("Unexpected failure - missing connection {}", id).into());
        }
    }

    Ok(())
}

#[async_trait]
impl Adapter for WickedAdapter {
    async fn read(&self) -> Result<model::NetworkState, Box<dyn std::error::Error>> {
        let interfaces = wicked_read(self.paths.clone())?;
        let settings = MIGRATION_SETTINGS.get().unwrap();
        let mut parents: HashMap<String, String> = HashMap::new();

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
                        connection_result.connection.id
                    )
                    .into());
                }
            }

            if let Some(parent) = interface.link.master {
                parents.insert(connection_result.connection.id.clone(), parent.clone());
            }
            state.add_connection(connection_result.connection)?;
        }

        update_parent_connection(&mut state, parents)?;

        Ok(state)
    }

    async fn write(
        &self,
        _network: &model::NetworkState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        unimplemented!("not needed");
    }
}

pub async fn migrate(paths: Vec<String>) -> Result<(), Box<dyn Error>> {
    let wicked = WickedAdapter::new(paths);
    let state = wicked.read().await?;
    let settings = MIGRATION_SETTINGS.get().unwrap();
    if settings.dry_run {
        for connection in state.connections {
            log::debug!("{:#?}", connection);
        }
        return Ok(());
    }
    let nm = NetworkManagerAdapter::from_system().await?;
    nm.write(&state).await?;
    Ok(())
}
