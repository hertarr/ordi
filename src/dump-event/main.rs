use std::fs::File;
use std::path::PathBuf;

use ordi::block::{InscribeEntry, TransferEntry};
use simplelog::*;

fn main() -> anyhow::Result<()> {
    let _ = dotenv::dotenv();
    let inscribe = |entry: InscribeEntry| {
        println!(
            "inscribe {}, {} at {}:{}.",
            entry.id, &entry.inscription_id, &entry.txid, entry.vout
        );
    };

    let transfer = |entry: TransferEntry| {
        println!(
            "transfer {} from {}:{} to {}:{}:{}.",
            entry.inscription_id,
            entry.from_output,
            entry.from_offset,
            entry.txid,
            entry.vout,
            entry.offset
        );
    };

    let ordi_data_dir = PathBuf::from(std::env::var("ordi_data_dir")?.as_str());
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Info,
        Config::default(),
        File::create(ordi_data_dir.join("debug.log")).unwrap(),
    )])?;

    let mut ordi = ordi::Ordi::new(false)?;
    ordi.when_inscribe(inscribe);
    ordi.when_transfer(transfer);

    if std::env::var("index_previous_output_value")? == "true" {
        ordi.index_output_value()?;
    }

    ordi.start().expect("Error happened when ordi is running.");

    ordi.close();

    Ok(())
}
