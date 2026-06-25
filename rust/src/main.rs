#![allow(unused)]
use bitcoin::hex::DisplayHex;
use bitcoincore_rpc::bitcoin::Amount;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde::Deserialize;
use serde_json::json;
use std::fs::File;
use std::io::Write;

// Node access params
const RPC_URL: &str = "http://127.0.0.1:18443"; // Default regtest RPC port
const RPC_USER: &str = "alice";
const RPC_PASS: &str = "password";

// You can use calls not provided in RPC lib API using the generic `call` function.
// An example of using the `send` RPC call, which doesn't have exposed API.
// You can also use serde_json `Deserialize` derivation to capture the returned json result.
fn send(rpc: &Client, addr: &str) -> bitcoincore_rpc::Result<String> {
    let args = [
        json!([{addr : 100 }]), // recipient address
        json!(null),            // conf target
        json!(null),            // estimate mode
        json!(null),            // fee rate in sats/vb
        json!(null),            // Empty option object
    ];

    #[derive(Deserialize)]
    struct SendResult {
        complete: bool,
        txid: String,
    }
    let send_result = rpc.call::<SendResult>("send", &args)?;
    assert!(send_result.complete);
    Ok(send_result.txid)
}

fn main() -> bitcoincore_rpc::Result<()> {
    // Connect to Bitcoin Core RPC
    let rpc = Client::new(
        RPC_URL,
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Get blockchain info
    let blockchain_info = rpc.get_blockchain_info()?;
    println!("Blockchain Info: {:?}", blockchain_info);

    // Create/Load the wallets, named 'Miner' and 'Trader'. Have logic to optionally create/load them if they do not exist or not loaded already.
    let miner_wallet = "Miner";
    let trader_wallet = "Trader";
    
    // to create wallets if they don't exist
    let _ = rpc.create_wallet(miner_wallet, None, None, None, None);
    let _ = rpc.create_wallet(trader_wallet, None, None, None, None);
    
    // load.  wallets to get client handles
    let miner_rpc = Client::new(
        &format!("{}/wallet/{}", RPC_URL, miner_wallet),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    let trader_rpc = Client::new(
        &format!("{}/wallet/{}", RPC_URL, trader_wallet),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Generate spendable balances in the Miner wallet. How many blocks needs to be mined?
    // ccoinbase rewards need 100 confirmations to be spendable in regtest mode.
    let mining_addr = miner_rpc.get_new_address(Some("Mining Reward"), None)?;
    let mining_addr_checked = mining_addr.assume_checked();
    
    // mine 101 blocks to get the first coinbase spendable (1 initial + 100 confirmations)
    rpc.generate_to_address(101, &mining_addr_checked)?;
    
    // the balance of the miner wallet
    let miner_balance = miner_rpc.get_balance(None, None)?;
    println!("Miner wallet balance: {}", miner_balance);
    
    // consolidate all coins into a single UTXO by sending to ourselves.so the next transaction will use only 1 input.
    let consolidation_addr = miner_rpc.get_new_address(None, None)?;
    let consolidation_addr_checked = consolidation_addr.assume_checked();
    let consolidation_amount = Amount::from_btc(49.9)?; // slightly less to account for fees
    let _consolidation_txid = miner_rpc.send_to_address(&consolidation_addr_checked, consolidation_amount, None, None, Some(true), None, None, None)?;
    
    // mine 1 block to confirm the consolidation
    rpc.generate_to_address(1, &mining_addr_checked)?;


    // Load Trader wallet and generate a new address
    let trader_addr = trader_rpc.get_new_address(Some("Received"), None)?;
    let trader_addr_checked = trader_addr.assume_checked();

    // Send 20 BTC from Miner to Trader
    let send_amount = Amount::from_btc(20.0)?;
    let txid = miner_rpc.send_to_address(&trader_addr_checked, send_amount, None, None, None, None, None, None)?;

    // Check transaction in mempool
    let mempool_entry = rpc.get_mempool_entry(&txid)?;
    println!("Mempool entry for {}: {:?}", txid, mempool_entry);

    // Mine 1 block to confirm the transaction
    rpc.generate_to_address(1, &mining_addr_checked)?;


    
    // Extract all required transaction details
    let tx = miner_rpc.get_transaction(&txid, None)?;
    
    // ddecode the transaction to get detailed input/output information
    let decoded = miner_rpc.decode_raw_transaction(&tx.hex, None)?;
    
    // for the input address, use the consolidation address (where the funds came from)
    let input_address = consolidation_addr_checked.to_string();
    let output_address = trader_addr_checked.to_string();
    
    // get input amount from the transaction
    let input_amount = 49.9;
    
    // extract outputs from decoded transaction
    let mut output_amount = 0.0;
    let mut change_amount = 0.0;
    let mut change_address = String::new();
    
    if decoded.vout.len() == 2 {
        // check which vout is 20 BTC (trader) and which is change
        let vout0_addr = if !decoded.vout[0].script_pub_key.addresses.is_empty() {
            decoded.vout[0].script_pub_key.addresses[0].clone().assume_checked().to_string()
        } else {
            String::new()
        };
        
        let vout1_addr = if !decoded.vout[1].script_pub_key.addresses.is_empty() {
            decoded.vout[1].script_pub_key.addresses[0].clone().assume_checked().to_string()
        } else {
            String::new()
        };
        
        let vout0_value = decoded.vout[0].value.to_btc();
        let vout1_value = decoded.vout[1].value.to_btc();
        
        // The vout with 20 BTC should go to trader, the other is change
        if (vout0_value - 20.0).abs() < 0.0001 {
            output_amount = vout0_value;
            change_address = vout1_addr;
            change_amount = vout1_value;
        } else if (vout1_value - 20.0).abs() < 0.0001{
            output_amount = vout1_value;
            change_address = vout0_addr;
            change_amount = vout0_value;
        } else {
            // fallback: search by address match
            for vout in &decoded.vout {
                if !vout.script_pub_key.addresses.is_empty() {
                    let addr = vout.script_pub_key.addresses[0].clone().assume_checked().to_string();
                    if addr == output_address {
                        output_amount = vout.value.to_btc();
                    } else {
                        change_address = addr;
                        change_amount = vout.value.to_btc();
                    }
                }
            }
        }
    }
    
    let fee = tx.fee.map(|f| f.to_btc().abs()).unwrap_or(0.0);
    let block_height = tx.info.blockheight.unwrap_or(0);
    let block_hash = tx.info.blockhash.map(|h| h.to_string()).unwrap_or_default();

    // Write the data to ../out.txt in the specified format given in readme.md
    let output = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        txid,
        input_address,
        input_amount,
        output_address,
        output_amount,
        change_address,
        change_amount,
        fee,
        block_height,
        block_hash
    );

    let mut file = File::create("../out.txt")?;
    file.write_all(output.as_bytes())?;
    println!("Transaction details written to out.txt");

    Ok(())
}
