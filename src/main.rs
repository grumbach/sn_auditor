
use std::path::Path;
use color_eyre::eyre::{Result, eyre};
use sn_client::{Client, SpendDag};
use sn_transfers::{SpendAddress, GENESIS_CASHNOTE};
use tiny_http::{Response, Server};
use graphviz_rust::{
    cmd::Format,
    exec,
    printer::PrinterContext, parse,
};

const SPEND_DAG_FILENAME: &str = "spend_dag";

#[tokio::main]
async fn main() {
    println!("Connecting to Network...");
    let client = Client::quick_start(None).await.expect("Could not create client");

    println!("Gather Spend DAG...");
    let path = dirs_next::data_dir()
        .expect("could not obtain data directory path")
        .join("safe")
        .join("auditor");
    let dag = gather_spend_dag(&client, &path).await.expect("Error gathering spend dag");

    println!("Starting server...");
    start_server(&client, &path, dag).await.expect("Error starting server");
}

async fn start_server(client: &Client, path: &Path, mut dag: SpendDag) -> Result<()> {
    let server = Server::http("0.0.0.0:4242").expect("Failed to start server");
    println!("Starting http server listening on port 4242...");
    for request in server.incoming_requests() {
        println!("Received request! method: {:?}, url: {:?}",
            request.method(),
            request.url(),
        );
        if request.url() != "/spend_dag.svg" {
            let response = Response::from_string("try GET /spend_dag.svg to get the spend DAG as a SVG image.");
            let _ = request.respond(response).map_err(|err| {
                eprintln!("Failed to send response: {err}");
            });
            continue;
        }

        println!("Update DAG...");
        let dag_path = path.join(SPEND_DAG_FILENAME);
        let svg_path = path.join("spend_dag.svg");
        client.spend_dag_continue_from_utxos(&mut dag).await?;
        dag.dump_to_file(dag_path)?;

        let svg = dag_to_svg(&dag).map_err(|err| {
            eprintln!("Failed to convert dag to svg: {err}");
        }).unwrap_or("Failed to convert dag to svg".as_bytes().to_vec());
        let _ = std::fs::write(&svg_path, &svg).map_err(|err| {
            eprintln!("Failed to write svg to file: {err}");
        });

        let response = Response::from_data(svg);
        let _ = request.respond(response).map_err(|err| {
            eprintln!("Failed to send response: {err}");
        });
    }
    Ok(())
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

async fn gather_spend_dag(client: &Client, root_dir: &Path) -> Result<SpendDag> {
    let dag_path = root_dir.join(SPEND_DAG_FILENAME);
    let dag = match SpendDag::load_from_file(&dag_path) {
        Ok(mut dag) => {
            println!("Found a local spend DAG file, updating it with latest Network Spends...");
            client.spend_dag_continue_from_utxos(&mut dag).await?;
            dag
        }
        Err(_) => {
            println!("Starting from Genesis...");
            let genesis_addr = SpendAddress::from_unique_pubkey(&GENESIS_CASHNOTE.unique_pubkey());
            client.spend_dag_build_from(genesis_addr).await?
        }
    };

    println!("Creating a local backup to disk...");
    std::fs::create_dir_all(root_dir)?;
    dag.dump_to_file(dag_path)?;

    Ok(dag)
}
