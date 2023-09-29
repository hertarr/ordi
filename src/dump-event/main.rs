use ordi::*;

fn main() -> anyhow::Result<()> {
    let _ = dotenv::dotenv();

    let mut ordi = Ordi::new(Options::default())?;

    ordi.when_inscribe(|entry| {
        println!(
            "inscribe {}, {} at {}:{}.",
            entry.id, &entry.inscription_id, &entry.txid, entry.vout
        );
    });

    ordi.when_transfer(|entry| {
        println!(
            "transfer {} from {}:{} to {}:{}:{}.",
            entry.inscription_id,
            entry.from_output,
            entry.from_offset,
            entry.txid,
            entry.vout,
            entry.offset
        );
    });

    // If index_previous_output_value is set true,
    // dump-event would reindex utxos until height 767430.
    // else use rpc to get utxo like ord.
    if std::env::var("index_previous_output_value")? == "true" {
        ordi.index_output_value()?;
    }

    ordi.start().expect("Error happened when ordi is running.");

    ordi.close();

    Ok(())
}
