// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::eyre::{eyre, Result};
use graphviz_rust::{cmd::Format, exec, parse, printer::PrinterContext};
use sn_client::{Client, SpendDag};
use sn_transfers::{SpendAddress, GENESIS_CASHNOTE};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

const SPEND_DAG_FILENAME: &str = "spend_dag";

/// Abstraction for the Spend DAG database
/// Currently in memory, with disk backup, but should probably be a real DB at scale
#[derive(Clone)]
pub struct SpendDagDb {
    client: Client,
    path: PathBuf,
    dag: Arc<RwLock<SpendDag>>,
}

impl SpendDagDb {
    /// Create a new SpendDagDb
    /// If a local spend DAG file is found, it will be loaded
    /// Else a new DAG will be created containing only Genesis
    pub async fn new(path: PathBuf, client: Client) -> Result<Self> {
        let dag_path = path.join(SPEND_DAG_FILENAME);
        let dag = match SpendDag::load_from_file(&dag_path) {
            Ok(d) => {
                println!("Found a local spend DAG file");
                d
            }
            Err(_) => {
                println!("Found no local spend DAG file, starting from Genesis");
                new_dag_with_genesis_only(&client).await?
            }
        };

        Ok(Self {
            client,
            path,
            dag: Arc::new(RwLock::new(dag)),
        })
    }

    /// Get info about a single spend in JSON format
    pub fn spend_json(&self, addr: SpendAddress) -> Result<String> {
        let dag_ref = self.dag.clone();
        let r_handle = dag_ref
            .read()
            .map_err(|e| eyre!("Failed to get read lock: {e}"))?;
        let spend = r_handle.get_spend(&addr);
        let json = serde_json::to_string_pretty(&spend)?;
        Ok(json)
    }

    /// Dump DAG to disk
    pub fn dump(&self) -> Result<()> {
        std::fs::create_dir_all(&self.path)?;
        let dag_path = self.path.join(SPEND_DAG_FILENAME);
        let dag_ref = self.dag.clone();
        let r_handle = dag_ref
            .read()
            .map_err(|e| eyre!("Failed to get read lock: {e}"))?;
        r_handle.dump_to_file(dag_path)?;
        Ok(())
    }

    /// Get the current DAG as SVG
    pub fn svg(&self) -> Result<Vec<u8>> {
        let dag_ref = self.dag.clone();
        let r_handle = dag_ref
            .read()
            .map_err(|e| eyre!("Failed to get read lock: {e}"))?;
        dag_to_svg(&r_handle)
    }

    /// Update DAG from Network
    pub async fn update(&mut self) -> Result<()> {
        // read current DAG
        let mut dag = {
            self.dag
                .clone()
                .read()
                .map_err(|e| eyre!("Failed to get read lock: {e}"))?
                .clone()
        };

        // update that copy
        self.client.spend_dag_continue_from_utxos(&mut dag).await?;

        // write update to DAG
        let dag_ref = self.dag.clone();
        let mut w_handle = dag_ref
            .write()
            .map_err(|e| eyre!("Failed to get write lock: {e}"))?;
        *w_handle = dag;

        Ok(())
    }

    /// Merge a SpendDag into the current DAG
    /// This can be used to enrich our DAG with a DAG from another node to avoid costly computations
    /// Make sure to verify the other DAG is trustworthy before calling this function to merge it in
    pub fn merge(&mut self, other: SpendDag) -> Result<()> {
        let dag_ref = self.dag.clone();
        let mut w_handle = dag_ref
            .write()
            .map_err(|e| eyre!("Failed to get write lock: {e}"))?;
        w_handle.merge(other);
        Ok(())
    }
}

pub async fn new_dag_with_genesis_only(client: &Client) -> Result<SpendDag> {
    let genesis_addr = SpendAddress::from_unique_pubkey(&GENESIS_CASHNOTE.unique_pubkey());
    let mut dag = SpendDag::new();
    let genesis_spend = match client.get_spend_from_network(genesis_addr).await {
        Ok(s) => s,
        Err(sn_client::Error::DoubleSpend(addr, spend1, spend2)) => {
            println!("Double spend detected at Genesis: {addr:?}");
            dag.insert(genesis_addr, *spend2);
            *spend1
        }
        Err(e) => return Err(eyre!("Failed to get genesis spend: {e}")),
    };
    dag.insert(genesis_addr, genesis_spend);

    Ok(dag)
}

fn dag_to_svg(dag: &SpendDag) -> Result<Vec<u8>> {
    let dot = dag.dump_dot_format();
    let graph = parse(&dot).map_err(|err| eyre!("Failed to parse dag from dot: {err}"))?;
    let graph_svg = exec(
        graph,
        &mut PrinterContext::default(),
        vec![Format::Svg.into()],
    )?;
    Ok(graph_svg)
}
