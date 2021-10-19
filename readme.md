
Extra assumptions I made not in requirements:

1. duplicated deposit/withdrawal transactions with the same tx id as a previous one are assumed invalid and skipped
2. dispute/resolve/chargeback tx ids where the client id doesn't match the original transaction client's id are assumed invalid and skipped
3. deposit/withdrawal transactions that are 0 or negative are assumed to be invalid and skipped
4. withdrawals for a client that is locked are assumed to be invalid and skipped, other transactions for locked clients are allowed
5. any transactions that result in any integer overflow are assumed to be invalid and skipped
6. withdrawals can be disputed, which can result in negative holds
7. A deposit/withdrawal can change between disputed/resolved unlimited times, but once it goes to chargeback, this is final as requirements say chargeback is the final state
8. A chargeback is only valid if the transaction is currently disputed, otherwise it's skipped
9. A resolve is only valid if the transaction is currently disputed, otherwise it's skipped
10. A dispute is only valid if the transaction hasn't been disputed/chargebacked or has been resolved
11. dispute/resolve/chargeback rows with an amount are assumed to be invalid and skipped
12. csv input files are valid utf-8 only

Code Structure:

1. TransactionReader that provides a stream of valid transactions, as much as they can be validated stand-alone, 
(ie duplicate transactions aren't detected at this step), this is tested stand-alone and will handle unlimited-size streams, as it only
constructs 1 valid row at a time for processing
2. TransactionEngine that consumes 1 validated transaction at a time, the rust type system enforces it only accepts valid transactions.
If the transaction is invalid in the context of past transactions, the method returns false and does not make any changes to the
application state.  At any point an Iterator over the list of Client accounts can be retrieved and examined.  Since this
has to maintain a list of all previous deposit/withdrawal Transactions (so disputes/resolves/chargebacks can be handled),
and all Client accounts, it will be limited by available memory.  In a production system this would be backed by a database.
3. Method to print an Iterator of Client accounts to a Writer as CSV
4. main method which stitches the above 3 together to read+process an input file, and print the client accounts to stdout.
5. a unit test runs the full csv to csv pipeline and compares to an expected result in memory, I've tried to test all possible
cases and commented my tests as needed.

In addition to the csv/serde crates, I also added rust_decimal which implements integer math for calculations involving money,
because it's inappropriate to use floats for money, and this didn't seem worth implementing myself.
