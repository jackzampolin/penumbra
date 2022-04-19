use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use std::env;

#[allow(clippy::clone_on_copy)]
use std::path::PathBuf;

use directories::ProjectDirs;

use structopt::StructOpt;

// use command::*;
// use state::ClientStateFile;
// use sync::sync;

use penumbra_wallet_next::{insert_table, read_table};
#[derive(Debug, StructOpt)]
#[structopt(
    name = "pcli",
    about = "The Penumbra command-line interface.",
    version = env!("VERGEN_GIT_SEMVER"),
)]
pub struct Opt {
    /// The address of the pd+tendermint node.
    #[structopt(short, long, default_value = "testnet.penumbra.zone")]
    pub node: String,
    /// The port to use to speak to tendermint.
    #[structopt(short, long, default_value = "26657")]
    pub rpc_port: u16,
    /// The port to use to speak to pd's light wallet server.
    #[structopt(short, long, default_value = "26666")]
    pub oblivious_query_port: u16,
    /// The port to use to speak to pd's thin wallet server.
    #[structopt(short, long, default_value = "26667")]
    pub specific_query_port: u16,
    /// The location of the wallet file [default: platform appdata directory]
    #[structopt(short, long)]
    pub wallet_location: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let opt = Opt::from_args();

    let project_dir =
        ProjectDirs::from("zone", "penumbra", "pcli").expect("can access penumbra project dir");
    // Currently we use just the data directory. Create it if it is missing.
    std::fs::create_dir_all(project_dir.data_dir()).expect("can create penumbra data directory");

    // We store wallet data in `penumbra_wallet.dat` in the state directory, unless
    // the user provides another location.
    // TODO: make this not depend on project directory to support other custody services
    let wallet_path = opt.wallet_location.as_ref().map_or_else(
        || project_dir.data_dir().join("penumbra_wallet.json"),
        PathBuf::from,
    );

    // Synchronize the wallet if the command requires it to be synchronized before it is run.
    let mut state = ClientStateFile::load(wallet_path.clone())?;

    // Chain params may not have been fetched yet, do so if necessary.
    if state.chain_params().is_none() {
        fetch::chain_params(&opt, &mut state).await?;
    }
    // From now on, we can .expect() on the chain params.

    // Always sync pwalletd on startup.
    sync(&opt, &mut state).await?;
    fetch::assets(&opt, &mut state).await?;

    let pool = SqlitePool::connect(&env::var("DATABASE_URL")?).await?;

    sqlx::migrate!().run(&pool).await?;
    let row = insert_table(&pool).await?;
    let x = read_table(&pool).await?;

    println!(
        "Hello, pwalletd! I got stuff from sqlite: row {} value {}",
        row, x
    );

    // TODO: start gRPC service and respond to command requests
    let wallet_server = tokio::spawn(
        Server::builder()
            .trace_fn(|req| match remote_addr(req) {
                Some(remote_addr) => tracing::error_span!("wallet_query", ?remote_addr),
                None => tracing::error_span!("wallet_query"),
            })
            .add_service(WalletServer::new(storage.clone()))
            .serve(
                format!("{}:{}", host, wallet_query_port)
                    .parse()
                    .expect("this is a valid address"),
            ),
    );
    Ok(())
}

#[instrument(skip(opt, state), fields(start_height = state.last_block_height()))]
pub async fn sync(opt: &Opt, state: &mut ClientStateFile) -> Result<()> {
    tracing::info!("starting client sync");
    let mut client = opt.oblivious_client().await?;

    let start_height = state.last_block_height().map(|h| h + 1).unwrap_or(0);
    let mut stream = client
        .compact_block_range(tonic::Request::new(CompactBlockRangeRequest {
            start_height,
            end_height: 0,
            chain_id: state
                .chain_id()
                .ok_or_else(|| anyhow::anyhow!("missing chain_id"))?,
        }))
        .await?
        .into_inner();

    let mut count = 0;
    while let Some(block) = stream.message().await? {
        state.scan_block(block.try_into()?)?;
        // very basic form of intermediate checkpointing
        count += 1;
        if count % 1000 == 1 {
            state.commit()?;
            tracing::info!(height = ?state.last_block_height().unwrap(), "syncing...");
        }
    }

    state.prune_timeouts();
    state.commit()?;
    tracing::info!(end_height = ?state.last_block_height().unwrap(), "finished sync");
    Ok(())
}
