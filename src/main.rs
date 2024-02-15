// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod dag_db;

use dag_db::SpendDagDb;

use color_eyre::eyre::{eyre, Result};
use sn_client::Client;
use tiny_http::{Response, Server};

#[tokio::main]
async fn main() {
    println!("Connecting to Network...");
    let client = Client::quick_start(None)
        .await
        .expect("Could not create client");

    println!("Gather Spend DAG...");
    let path = dirs_next::data_dir()
        .expect("could not obtain data directory path")
        .join("safe")
        .join("auditor");
    let dag = dag_db::SpendDagDb::new(path.clone(), client.clone())
        .await
        .expect("Could not create SpendDagDb");

    println!("Starting background DAG collection thread...");
    // spawn a thread to collect the DAG in the background
    let mut d = dag.clone();
    tokio::spawn(async move {
        loop {
            println!("Updating DAG...");
            d.update().await.expect("Could not update DAG");
            d.dump().expect("Could not dump DAG to disk");
            println!("Updated DAG! Sleeping for 60 seconds...");
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }
    });

    // start the server
    println!("Starting server...");
    start_server(dag).await.expect("Error starting server");
}

async fn start_server(dag: SpendDagDb) -> Result<()> {
    let server = Server::http("0.0.0.0:4242").expect("Failed to start server");
    println!("Starting http server listening on port 4242...");
    for request in server.incoming_requests() {
        println!(
            "Received request! method: {:?}, url: {:?}",
            request.method(),
            request.url(),
        );
        if request.url() != "/spend_dag.svg" {
            let response = Response::from_string(
                "try GET /spend_dag.svg to get the spend DAG as a SVG image.",
            );
            let _ = request.respond(response).map_err(|err| {
                eprintln!("Failed to send response: {err}");
            });
            continue;
        }

        let svg = dag.svg().map_err(|e| eyre!("Failed to get SVG: {e}"))?;
        let response = Response::from_data(svg);
        let _ = request.respond(response).map_err(|err| {
            eprintln!("Failed to send response: {err}");
        });
    }
    Ok(())
}
