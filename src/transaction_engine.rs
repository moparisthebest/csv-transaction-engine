use std::collections::hash_map::{Entry, Values};
use std::collections::HashMap;

use crate::TransactionState::*;
use crate::{Client, Transaction, TransactionRow};

#[derive(Debug)]
pub struct TransactionEngine {
    // in production, we'd be using a real database instead of HashMaps
    transactions: HashMap<u32, Transaction>,
    clients: HashMap<u16, Client>,
}

impl Default for TransactionEngine {
    fn default() -> Self {
        TransactionEngine {
            transactions: HashMap::new(),
            clients: HashMap::new(),
        }
    }
}

impl TransactionEngine {
    /// returns true if the transaction successfully applied, and false otherwise
    /// if false is returned, then no modification happened at all
    /// if this was production code, this would return a Result with a proper Error that the client could act on
    pub fn apply(&mut self, tx: TransactionRow) -> bool {
        match tx {
            TransactionRow::New(tx) => {
                if let Entry::Vacant(tx_entry) = self.transactions.entry(tx.tx) {
                    // new transaction, but it can still be invalid if it's withdrawal for a client that does not exist or does not have enough available funds
                    // now insert or update the client
                    match self.clients.get_mut(&tx.client) {
                        None => {
                            // client does not exist
                            if tx.amount.is_sign_negative() {
                                // withdrawals for a new client are not allowed
                                return false;
                            }
                            self.clients
                                .insert(tx.client, Client::new(tx.client, tx.amount));
                        }
                        Some(client) => {
                            if client.locked && tx.amount.is_sign_negative() {
                                // withdrawals are not allowed for locked accounts
                                return false;
                            }
                            let available = client.available().checked_add(tx.amount);
                            if available.is_none() || available.unwrap().is_sign_negative() {
                                // withdrawals that overflow or will put the available balance into negative are not allowed
                                return false;
                            }
                            match client.total.checked_add(tx.amount) {
                                None => return false, // fail transactions that overflow
                                Some(new_total) => {
                                    if new_total.is_sign_negative() {
                                        // withdrawals that will put the total balance into negative are not allowed
                                        // this could happen because a withdrawal is disputed
                                        return false;
                                    }
                                    client.total = new_total;
                                }
                            }
                        }
                    }
                    tx_entry.insert(tx);
                    return true;
                }
                // if the transaction already exists, we ignore this one, again in production this would be an error to log or something
                false
            }
            TransactionRow::Mod(tx) => {
                match self.transactions.get_mut(&tx.tx) {
                    None => false, // can't mod a non-existing transactions
                    Some(orig_tx) => {
                        if orig_tx.client != tx.client {
                            // an update for an existing transaction but with a different client? hacker! do not apply transaction
                            return false;
                        }
                        let mut client = self.clients.get_mut(&orig_tx.client).unwrap(); // this unwrap is safe because we never insert a transaction without making sure the client exists first
                        match tx.state {
                            Disputed => {
                                if orig_tx.state != Resolved {
                                    // can only switch to Disputed from Resolved, otherwise this is invalid
                                    return false;
                                }
                                match client.held.checked_add(orig_tx.amount) {
                                    None => return false, // fail on overflow
                                    Some(held) => client.held = held,
                                }
                                orig_tx.state = tx.state;
                                true
                            }
                            Resolved => {
                                if orig_tx.state != Disputed {
                                    // can only switch to Resolved from Disputed, otherwise this is invalid
                                    return false;
                                }
                                match client.held.checked_sub(orig_tx.amount) {
                                    None => return false, // fail on overflow
                                    Some(held) => client.held = held,
                                }
                                orig_tx.state = tx.state;
                                true
                            }
                            Chargeback => {
                                if orig_tx.state != Disputed {
                                    // can only switch to Chargeback from Disputed, otherwise this is invalid
                                    return false;
                                }
                                match (
                                    client.held.checked_sub(orig_tx.amount),
                                    client.total.checked_sub(orig_tx.amount),
                                ) {
                                    (Some(held), Some(total)) => {
                                        client.held = held;
                                        client.total = total;
                                    }
                                    (_, _) => return false, // fail on overflow of either
                                }
                                orig_tx.state = tx.state;
                                client.locked = true;
                                true
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn clients(&self) -> Values<'_, u16, Client> {
        self.clients.values()
    }
}
