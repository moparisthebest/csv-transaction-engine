use std::fs::File;

use crate::transaction_engine::TransactionEngine;
use crate::transaction_reader::TransactionReader;
use crate::TransactionState::{Chargeback, Disputed, Resolved};
use rust_decimal::Decimal;
use serde::Serialize;

mod transaction_engine;
mod transaction_reader;

// number of places past the decimal to support
pub const DECIMAL_PLACES: u32 = 4;

#[derive(Debug, PartialEq)]
pub struct Transaction {
    tx: u32,
    client: u16,
    amount: Decimal, // Deposit is positive, Withdrawal is negative
    state: TransactionState,
}

#[derive(Debug, PartialEq)]
pub enum TransactionState {
    // we assume the state can flip back and forth between Disputed and Resolved unlimited times
    // but Chargeback is final
    Resolved, // the default case, or Resolved after a Dispute
    Disputed,
    Chargeback, // final state, all future transactions modifying this will be ignored
}

#[derive(Debug, PartialEq)]
pub struct TransactionMod {
    tx: u32,
    client: u16,
    state: TransactionState,
}

#[derive(Debug, PartialEq)]
pub enum TransactionRow {
    New(Transaction),
    Mod(TransactionMod),
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Client {
    client: u16,
    total: Decimal,
    held: Decimal,
    locked: bool,
}

impl Client {
    fn new(client: u16, total: Decimal) -> Client {
        Client {
            client,
            total,
            held: Decimal::new(0, DECIMAL_PLACES),
            locked: false,
        }
    }

    fn available(&self) -> Decimal {
        self.total - self.held
    }
}

pub fn dump_client_csv<'a, W: std::io::Write>(
    wtr: W,
    clients: impl Iterator<Item = &'a Client>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut wtr = csv::Writer::from_writer(wtr);
    wtr.write_record(&["client", "available", "held", "total", "locked"])?;
    for client in clients {
        wtr.write_record(&[
            client.client.to_string(),
            client.available().to_string(),
            client.held.to_string(),
            client.total.to_string(),
            client.locked.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn main() {
    let input_file = std::env::args_os()
        .nth(1)
        .expect("first argument must be CSV file");
    let input_file = File::open(input_file).expect("could not open CSV file");

    let mut tx_reader = TransactionReader::from_reader(input_file);
    let mut tx_engine = TransactionEngine::default();
    for tx_row in tx_reader.valid_records() {
        tx_engine.apply(tx_row);
    }

    // could sort clients here before output, but reqs say order does not matter
    dump_client_csv(std::io::stdout(), tx_engine.clients())
        .expect("cannot write to stdout? (should never happen)");
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_full_engine() {
        // we aren't going to bother testing invalid records here, because we already test they aren't included in transaction_reader tests
        let input_file = b"\
type, client, tx, amount
deposit, 1, 1, 1.0
deposit, 2, 2, 2.0
deposit, 3, 3, 3.0
# next deposit will be ignored because it's a duplicate tx id
deposit, 1, 3, 2.0
# this withdrawal will be ignored too for duplicate tx id
withdrawal, 1, 2, 1.0
# withdrawal for non-existent client will fail
withdrawal, 100, 4, 1.0
# non-sequential tx ids are fine
withdrawal, 3, 50, 1.0
# non-sequential client ids are fine too
deposit, 50, 51, 50.5555

# now let's dispute
deposit, 2, 5, 5.0
# a chargeback when in the resolved state is ignored
chargeback, 2, 5,
dispute, 2, 5,
# a second dispute is ignored
dispute, 2, 5,
resolve, 2, 5,
# a chargeback when in the resolved state is ignored
chargeback, 2, 5,
# but a dispute and then chargeback is final
dispute, 2, 5,
chargeback, 2, 5,
# resolve will not work
resolve, 2, 5,

# even though client 2 has 2.000 left, withdrawal will fail due to the account being locked
withdrawal, 2, 6, 1.0
# but a deposit will work
deposit, 2, 7, 1.0
# a dispute against a deposit where the client id does not match the original is rejected
dispute, 3, 7,

# withdrawal where not enough funds are available will fail
withdrawal, 50, 8, 60
# outrageously large deposit works
deposit, 50, 19, 7922816251426433751
# deposit with overflow will fail
deposit, 50, 20, 792281625142643375172

";

        let expected_client_csv = b"\
client,available,held,total,locked
1,1.0000,0.0000,1.0000,false
2,3.0000,0.0000,3.0000,true
3,2.0000,0.0000,2.0000,false
50,7922816251426433801.5555,0.0000,7922816251426433801.5555,false
";

        let mut tx_reader = TransactionReader::from_reader(&input_file[..]);
        let mut tx_engine = TransactionEngine::default();
        for tx_row in tx_reader.valid_records() {
            tx_engine.apply(tx_row);
        }

        // we are going to sort it by client id because it needs ordered to compare it
        let mut clients: Vec<&Client> = tx_engine.clients().collect();
        clients.sort_by(|a, b| a.client.cmp(&b.client));

        let mut out: Vec<u8> = Vec::new();
        dump_client_csv(&mut out, clients.into_iter()).unwrap();

        // for debugging
        //use std::io::{stdout, Write};
        //stdout().write_all(&out).unwrap();

        assert_eq!(&expected_client_csv[..], &out)
    }
}
