# ordi 

`ordi` is a fast light indexer for building ordinals data source.

## Usage

```rust
export btc_data_dir=
export ordi_data_dir=

export btc_rpc_host=
export btc_rpc_user=
export btc_rpc_pass=

use ordi::*;

let mut ordi = Ordi::new(Options::default())?;
ordi.when_inscribe(inscribe_callback);
ordi.when_transfer(transfer_callback);
ordi.start()?;
ordi.close();
```

## Example

[dump-event](https://github.com/Hertarr/ordi/blob/master/src/dump-event/main.rs): use `.env` to export environments, check `.env.example`.

You could download [snapshot](https://drive.google.com/file/d/1ngrBDyRONQUFtF8SJtM8ZsJ5CQwy1CaO/view) for utxos at height 767430. Just unzip it into `ordi_data_dir` as folder `output_value`,
 And set environment `export index_previous_output_value=false`.

```
ordi_data_dir
|
--output_value
```

## Contributing
If you wish to contribute to `ordi`, feel free to create a pull request. If you feel unsure
about your plans, feel free to create an issue.

### Happy Coding!
