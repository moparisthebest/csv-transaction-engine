use std::convert::TryInto;
use std::ops::MulAssign;

use csv::{Reader, ReaderBuilder, Trim};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::*;

pub struct TransactionReader<R> {
    reader: Reader<R>,
}

impl<R: std::io::Read> TransactionReader<R> {
    pub fn from_reader(rdr: R) -> TransactionReader<R> {
        TransactionReader {
            reader: ReaderBuilder::new().trim(Trim::All).from_reader(rdr),
        }
    }

    // in a real application, you wouldn't just silently discard invalid records, but here we will
    pub fn valid_records(&mut self) -> ValidRecordsIter<R> {
        ValidRecordsIter {
            deserialize_records: self.reader.deserialize(),
        }
    }
}

pub struct ValidRecordsIter<'r, R: 'r> {
    deserialize_records: csv::DeserializeRecordsIter<'r, R, RawTransactionRow>,
}

impl<'r, R: std::io::Read> Iterator for ValidRecordsIter<'r, R> {
    type Item = TransactionRow;

    fn next(&mut self) -> Option<TransactionRow> {
        loop {
            match self.deserialize_records.next() {
                None => return None,
                Some(Ok(transaction_row)) => match transaction_row.try_into() {
                    Ok(transaction_row) => return Some(transaction_row),
                    Err(_) => continue,
                },
                _ => continue, // move to next on Err
            }
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum RawTransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

#[derive(Debug, Deserialize, PartialEq)]
struct RawTransactionRow {
    r#type: RawTransactionType,
    client: u16,
    tx: u32,
    amount: Option<Decimal>,
}

impl TryInto<TransactionRow> for RawTransactionRow {
    type Error = &'static str; // we aren't handling these anyway, real production code would and would need a better type

    fn try_into(self) -> Result<TransactionRow, Self::Error> {
        match self.r#type {
            RawTransactionType::Deposit | RawTransactionType::Withdrawal => {
                if let Some(mut amount) = self.amount {
                    // amount cannot be 0, negative, or have more than the allowed number of DECIMAL_PLACES
                    if amount.scale() <= DECIMAL_PLACES
                        && !amount.is_zero()
                        && !amount.is_sign_negative()
                    {
                        // valid amount, so valid deposit or withdrawal
                        amount.rescale(DECIMAL_PLACES);
                        if self.r#type == RawTransactionType::Withdrawal {
                            // a withdrawal is just a negative deposit
                            amount.mul_assign(Decimal::NEGATIVE_ONE);
                        }
                        return Ok(TransactionRow::New(Transaction {
                            tx: self.tx,
                            client: self.client,
                            amount,
                            state: Resolved,
                        }));
                    }
                }
                Err("missing or invalid amount")
            }
            RawTransactionType::Dispute
            | RawTransactionType::Resolve
            | RawTransactionType::Chargeback => match self.amount {
                Some(_) => Err("amount provided for Dispute/Resolve/Chargeback and not allowed"),
                None => Ok(TransactionRow::Mod(TransactionMod {
                    tx: self.tx,
                    client: self.client,
                    state: match self.r#type {
                        RawTransactionType::Dispute => Disputed,
                        RawTransactionType::Resolve => Resolved,
                        RawTransactionType::Chargeback => Chargeback,
                        _ => unreachable!("impossible to reach this due to outer match"),
                    },
                })),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Decimal;
    use crate::{
        Transaction, TransactionMod, TransactionReader, TransactionRow, TransactionRow::*,
        TransactionState::*,
    };
    use std::str::FromStr;

    #[test]
    fn read_valid_rows() {
        let input_file = b"\
type, client, tx, amount
deposit, 1, 1, 1.0
deposit, 2, 2, 2.0
deposit, 1, 3, 2.0
withdrawal, 1, 4, 1.5
withdrawal, 2, 5, 3.0
# our parser ignores bad lines, which is handy so we can add comments like these next 3 are bad on purpose :)
withdrawal, 2, 5,
withdrawal, 2, 5, bla
bad, 2, 5, 4.0
deposit, 4, 84, 0
deposit, 4, 84, 4
deposit, trash, 84, 5
deposit, 83, trash, 5
deposit, 2, 2, -2.1
withdrawal, 2, 2, -2.1
deposit, 2, 2, 2.000001
deposit, 2, 2, 2.00001
deposit, 2, 2, 2.0001
deposit, 2, 2, 2.001
deposit, 2, 2, 2.0010
deposit, 2, 2, 2.01
deposit, 2, 2, 2.1
deposit, 2, 2, 2
dispute, 2, 2, 5
dispute, 2, 2,
chargeback, 2, 2,
resolve, 2, 2,
";
        let mut rdr = TransactionReader::from_reader(&input_file[..]);
        let all_valid_records: Vec<TransactionRow> = rdr.valid_records().collect();
        fn dec(s: &str) -> Decimal {
            Decimal::from_str(s).unwrap()
        }

        #[rustfmt::skip]
        assert_eq!(all_valid_records, vec![
            New(Transaction { tx: 1, client: 1, amount: dec("1.0000"), state: Resolved }),
            New(Transaction { tx: 2, client: 2, amount: dec("2.0000"), state: Resolved }),
            New(Transaction { tx: 3, client: 1, amount: dec("2.0000"), state: Resolved }),
            New(Transaction { tx: 4, client: 1, amount: dec("-1.5000"), state: Resolved }),
            New(Transaction { tx: 5, client: 2, amount: dec("-3.0000"), state: Resolved }),
            New(Transaction { tx: 84, client: 4, amount: dec("4.0000"), state: Resolved }),
            New(Transaction { tx: 2, client: 2, amount: dec("2.0001"), state: Resolved }),
            New(Transaction { tx: 2, client: 2, amount: dec("2.0010"), state: Resolved }),
            New(Transaction { tx: 2, client: 2, amount: dec("2.0010"), state: Resolved }),
            New(Transaction { tx: 2, client: 2, amount: dec("2.0100"), state: Resolved }),
            New(Transaction { tx: 2, client: 2, amount: dec("2.1000"), state: Resolved }),
            New(Transaction { tx: 2, client: 2, amount: dec("2.0000"), state: Resolved }),
            Mod(TransactionMod { tx: 2, client: 2, state: Disputed }),
            Mod(TransactionMod { tx: 2, client: 2, state: Chargeback }),
            Mod(TransactionMod { tx: 2, client: 2, state: Resolved }),
        ]);
    }
}
