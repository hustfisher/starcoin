address 0x0 {

// The module for the account resource that governs every account
module Account {
    use 0x0::Association;
    use 0x0::Event;
    use 0x0::Hash;
    use 0x0::LCS;
    use 0x0::Coin;
    use 0x0::TransactionTimeout;
    use 0x0::Testnet;
    use 0x0::Transaction;
    use 0x0::Vector;
    use 0x0::Signer;
    use 0x0::Timestamp;

    // Every account has a Account::T resource
    resource struct T {
        // The current authentication key.
        // This can be different than the key used to create the account
        authentication_key: vector<u8>,
        // If true, the authority to rotate the authentication key of this account resides elsewhere
        delegated_key_rotation_capability: bool,
        // If true, the authority to withdraw funds from this account resides elsewhere
        delegated_withdrawal_capability: bool,
        // Event handle for received event
        received_events: Event::EventHandle<ReceivedPaymentEvent>,
        // Event handle for sent event
        sent_events: Event::EventHandle<SentPaymentEvent>,
        // The current sequence number.
        // Incremented by one each time a transaction is submitted
        sequence_number: u64,
        is_frozen: bool,
    }

    // A resource that holds the coins stored in this account
    resource struct Balance<Token> {
        coin: Coin::T<Token>,
    }

    // The holder of WithdrawalCapability for account_address can withdraw Libra from
    // account_address/Account::T/balance.
    // There is at most one WithdrawalCapability in existence for a given address.
    resource struct WithdrawalCapability {
        account_address: address,
    }

    // The holder of KeyRotationCapability for account_address can rotate the authentication key for
    // account_address (i.e., write to account_address/Account::T/authentication_key).
    // There is at most one KeyRotationCapability in existence for a given address.
    resource struct KeyRotationCapability {
        account_address: address,
    }

    // Message for sent events
    struct SentPaymentEvent {
        // The amount of Coin::T<Token> sent
        amount: u64,
        // The code symbol for the currency that was sent
        currency_code: vector<u8>,
        // The address that was paid
        payee: address,
        // Metadata associated with the payment
        metadata: vector<u8>,
    }

    // Message for received events
    struct ReceivedPaymentEvent {
        // The amount of Coin::T<Token> received
        amount: u64,
        // The code symbol for the currency that was received
        currency_code: vector<u8>,
        // The address that sent the coin
        payer: address,
        // Metadata associated with the payment
        metadata: vector<u8>,
    }

    // A privilege to allow the freezing of accounts.
    struct FreezingPrivilege { }


     public fun initialize(association: &signer) {
         Transaction::assert(Signer::address_of(association) == 0xA550C18, 0);
     }

    // Deposits the `to_deposit` coin into the `payee`'s account balance
    public fun deposit<Token>(account: &signer, payee: address, to_deposit: Coin::T<Token>)
    acquires T, Balance {
        // Since we don't have vector<u8> literals in the source language at
        // the moment.
        deposit_with_metadata(account, payee, to_deposit, x"", x"")
    }

    // Deposits the `to_deposit` coin into the sender's account balance
    public fun deposit_to_sender<Token>(account: &signer, to_deposit: Coin::T<Token>)
    acquires T, Balance {
        deposit(account, Signer::address_of(account), to_deposit)
    }

    // Deposits the `to_deposit` coin into the `payee`'s account balance with the attached `metadata`
    public fun deposit_with_metadata<Token>(account: &signer,
        payee: address,
        to_deposit: Coin::T<Token>,
        metadata: vector<u8>,
        metadata_signature: vector<u8>
    ) acquires T, Balance {
        deposit_with_sender_and_metadata(
            payee,
            Signer::address_of(account),
            to_deposit,
            metadata,
            metadata_signature
        );
    }

    // Deposits the `to_deposit` coin into the `payee`'s account balance with the attached `metadata` and
    // sender address
    fun deposit_with_sender_and_metadata<Token>(
        payee: address,
        sender: address,
        to_deposit: Coin::T<Token>,
        metadata: vector<u8>,
        _metadata_signature: vector<u8>
    ) acquires T, Balance {
        // Check that the `to_deposit` coin is non-zero
        let deposit_value = Coin::value(&to_deposit);
        Transaction::assert(deposit_value > 0, 7);

        //TODO check signature
        //Transaction::assert(Vector::length(&metadata_signature) == 64, 9001);
        // cryptographic check of signature validity
        //Transaction::assert(
        //    Signature::ed25519_verify(
        //        metadata_signature,
        //        VASP::travel_rule_public_key(payee),
        //        copy metadata
        //    ),
        //    9002, // TODO: proper error code
        //);

        // Get the code symbol for this currency
        let currency_code = Coin::currency_code<Token>();

        // Load the sender's account
        let sender_account_ref = borrow_global_mut<T>(sender);
        // Log a sent event
        Event::emit_event<SentPaymentEvent>(
            &mut sender_account_ref.sent_events,
            SentPaymentEvent {
                amount: deposit_value,
                currency_code: copy currency_code,
                payee: payee,
                metadata: *&metadata
            },
        );

        // Load the payee's account
        let payee_account_ref = borrow_global_mut<T>(payee);
        let payee_balance = borrow_global_mut<Balance<Token>>(payee);
        // Deposit the `to_deposit` coin
        Coin::deposit(&mut payee_balance.coin, to_deposit);
        // Log a received event
        Event::emit_event<ReceivedPaymentEvent>(
            &mut payee_account_ref.received_events,
            ReceivedPaymentEvent {
                amount: deposit_value,
                currency_code,
                payer: sender,
                metadata: metadata
            }
        );
    }

    // mint_to_address can only be called by accounts with MintCapability (see Libra)
    // and those accounts will be charged for gas. If those accounts don't have enough gas to pay
    // for the transaction cost they will fail minting.
    // However those account can also mint to themselves so that is a decent workaround
    public fun mint_to_address<Token>(
        account: &signer,
        payee: address,
        amount: u64
    ) acquires T, Balance {
        // Mint and deposit the coin
        deposit(account, payee, Coin::mint<Token>(account, amount));
    }

    // Cancel the oldest burn request from `preburn_address` and return the funds.
    // Fails if the sender does not have a published MintCapability.
    public fun cancel_burn<Token>(
        account: &signer,
        preburn_address: address,
    ) acquires T, Balance {
        let to_return = Coin::cancel_burn<Token>(account, preburn_address);
        deposit(account, preburn_address, to_return)
    }

    // Helper to withdraw `amount` from the given account balance and return the withdrawn Coin::T<Token>
    fun withdraw_from_balance<Token>(_addr: address, balance: &mut Balance<Token>, amount: u64): Coin::T<Token>{
        Coin::withdraw(&mut balance.coin, amount)
    }

    // Withdraw `amount` Coin::T<Token> from the transaction sender's account balance
    public fun withdraw_from_sender<Token>(account: &signer, amount: u64): Coin::T<Token>
    acquires T, Balance {
        let sender = Signer::address_of(account);
        let sender_account = borrow_global_mut<T>(sender);
        let sender_balance = borrow_global_mut<Balance<Token>>(sender);
        // The sender has delegated the privilege to withdraw from her account elsewhere--abort.
        Transaction::assert(!sender_account.delegated_withdrawal_capability, 11);
        // The sender has retained her withdrawal privileges--proceed.
        withdraw_from_balance<Token>(sender, sender_balance, amount)
    }

    // Withdraw `amount` Coin::T<Token> from the account under cap.account_address
    public fun withdraw_with_capability<Token>(
        cap: &WithdrawalCapability, amount: u64
    ): Coin::T<Token> acquires Balance {
        let balance = borrow_global_mut<Balance<Token>>(cap.account_address);
        withdraw_from_balance<Token>(cap.account_address, balance , amount)
    }

    // Return a unique capability granting permission to withdraw from the sender's account balance.
    public fun extract_sender_withdrawal_capability(account: &signer): WithdrawalCapability acquires T {
        let sender = Signer::address_of(account);
        let sender_account = borrow_global_mut<T>(sender);

        // Abort if we already extracted the unique withdrawal capability for this account.
        Transaction::assert(!sender_account.delegated_withdrawal_capability, 11);

        // Ensure the uniqueness of the capability
        sender_account.delegated_withdrawal_capability = true;
        WithdrawalCapability { account_address: sender }
    }

    // Return the withdrawal capability to the account it originally came from
    public fun restore_withdrawal_capability(cap: WithdrawalCapability) acquires T {
        // Destroy the capability
        let WithdrawalCapability { account_address } = cap;
        let account = borrow_global_mut<T>(account_address);
        // Update the flag for `account_address` to indicate that the capability has been restored.
        // The account owner will now be able to call pay_from_sender, withdraw_from_sender, and
        // extract_sender_withdrawal_capability again.
        account.delegated_withdrawal_capability = false;
    }

    // Withdraws `amount` Coin::T<Token> using the passed in WithdrawalCapability, and deposits it
    // into the `payee`'s account balance. Creates the `payee` account if it doesn't exist.
    public fun pay_from_capability<Token>(
        payee: address,
        cap: &WithdrawalCapability,
        amount: u64,
        metadata: vector<u8>,
        metadata_signature: vector<u8>
    ) acquires T, Balance {
        deposit_with_sender_and_metadata<Token>(
            payee,
            *&cap.account_address,
            withdraw_with_capability(cap, amount),
            metadata,
            metadata_signature
        );
    }

    // Withdraw `amount` Coin::T<Token> from the transaction sender's
    // account balance and send the coin to the `payee` address with the
    // attached `metadata` Creates the `payee` account if it does not exist
    public fun pay_from_sender_with_metadata<Token>(
        account: &signer,
        payee: address,
        amount: u64,
        metadata: vector<u8>,
        metadata_signature: vector<u8>
    ) acquires T, Balance {
        deposit_with_metadata<Token>(
            account,
            payee,
            withdraw_from_sender(account, amount),
            metadata,
            metadata_signature
        );
    }

    // Withdraw `amount` Coin::T<Token> from the transaction sender's
    // account balance  and send the coin to the `payee` address
    // Creates the `payee` account if it does not exist
    public fun pay_from_sender<Token>(
        account: &signer,
        payee: address,
        amount: u64
    ) acquires T, Balance {
        pay_from_sender_with_metadata<Token>(account, payee, amount, x"", x"");
    }

    fun rotate_authentication_key_for_account(account: &mut T, new_authentication_key: vector<u8>) {
      // Don't allow rotating to clearly invalid key
      Transaction::assert(Vector::length(&new_authentication_key) == 32, 12);
      account.authentication_key = new_authentication_key;
    }

    // Rotate the transaction sender's authentication key
    // The new key will be used for signing future transactions
    public fun rotate_authentication_key(account: &signer, new_authentication_key: vector<u8>) acquires T {
        let sender_account = borrow_global_mut<T>(Signer::address_of(account));
        // The sender has delegated the privilege to rotate her key elsewhere--abort
        Transaction::assert(!sender_account.delegated_key_rotation_capability, 11);
        // The sender has retained her key rotation privileges--proceed.
        rotate_authentication_key_for_account(
            sender_account,
            new_authentication_key
        );
    }

    // Rotate the authentication key for the account under cap.account_address
    public fun rotate_authentication_key_with_capability(
        cap: &KeyRotationCapability,
        new_authentication_key: vector<u8>,
    ) acquires T  {
        rotate_authentication_key_for_account(
            borrow_global_mut<T>(*&cap.account_address),
            new_authentication_key
        );
    }

    // Return a unique capability granting permission to rotate the sender's authentication key
    public fun extract_sender_key_rotation_capability(account: &signer): KeyRotationCapability acquires T {
        let sender = Signer::address_of(account);
        let sender_account = borrow_global_mut<T>(sender);
        // Abort if we already extracted the unique key rotation capability for this account.
        Transaction::assert(!sender_account.delegated_key_rotation_capability, 11);
        sender_account.delegated_key_rotation_capability = true; // Ensure uniqueness of the capability
        KeyRotationCapability { account_address: sender }
    }

    // Return the key rotation capability to the account it originally came from
    public fun restore_key_rotation_capability(cap: KeyRotationCapability) acquires T {
        // Destroy the capability
        let KeyRotationCapability { account_address } = cap;
        let account = borrow_global_mut<T>(account_address);
        // Update the flag for `account_address` to indicate that the capability has been restored.
        // The account owner will now be able to call rotate_authentication_key and
        // extract_sender_key_rotation_capability again
        account.delegated_key_rotation_capability = false;
    }

    // Create an account with the Empty role at `new_account_address` with authentication key
    /// `auth_key_prefix` | `new_account_address`
    // TODO: can we get rid of this? the main thing this does is create an account without an
    // EventGenerator resource (which is just needed to avoid circular dep issues in gensis)
    public fun create_genesis_account<Token>(
        new_account_address: address,
        auth_key_prefix: vector<u8>
    ) {
        Transaction::assert(Timestamp::is_genesis(), 0);
        let new_account = create_signer(new_account_address);
        make_account<Token>(new_account, auth_key_prefix)
    }

    // Creates a new testnet account at `fresh_address` with a balance of
    // zero `Token` type coins, and authentication key `auth_key_prefix` | `fresh_address`.
    // Trying to create an account at address 0x0 will cause runtime failure as it is a
    // reserved address for the MoveVM.
    public fun create_testnet_account<Token>(fresh_address: address, auth_key_prefix: vector<u8>){
        Transaction::assert(Testnet::is_testnet(), 10042);
        create_account<Token>(fresh_address, auth_key_prefix);
    }

    // Creates a new account at `fresh_address` with a balance of zero and authentication
    // key `auth_key_prefix` | `fresh_address`.
    // Creating an account at address 0x0 will cause runtime failure as it is a
    // reserved address for the MoveVM.
    public fun create_account<Token>(fresh_address: address, auth_key_prefix: vector<u8>){
        let new_account = create_signer(fresh_address);
        Event::publish_generator(&new_account);
        make_account<Token>(new_account, auth_key_prefix)
    }

    fun make_account<Token>(
        new_account: signer,
        auth_key_prefix: vector<u8>,
    ){
        let authentication_key = auth_key_prefix;
        let fresh_address = Signer::address_of(&new_account);
        Vector::append(&mut authentication_key, LCS::to_bytes(&fresh_address));
        Transaction::assert(Vector::length(&authentication_key) == 32, 12);
        move_to(&new_account, T {
              authentication_key,
              delegated_key_rotation_capability: false,
              delegated_withdrawal_capability: false,
              received_events: Event::new_event_handle<ReceivedPaymentEvent>(&new_account),
              sent_events: Event::new_event_handle<SentPaymentEvent>(&new_account),
              sequence_number: 0,
              is_frozen: false,
        });
        move_to(&new_account, Balance<Token>{coin: Coin::zero<Token>()});
        destroy_signer(new_account);
    }

    native fun create_signer(addr: address): signer;
    native fun destroy_signer(sig: signer);

    // Helper to return the u64 value of the `balance` for `account`
    fun balance_for<Token>(balance: &Balance<Token>): u64 {
        Coin::value<Token>(&balance.coin)
    }

    // Return the current balance of the account at `addr`.
    public fun balance<Token>(addr: address): u64 acquires Balance {
        balance_for(borrow_global<Balance<Token>>(addr))
    }
    //TODO use a unify name https://github.com/starcoinorg/starcoin/issues/570
    // Add a balance of `Token` type to the sending account.
    public fun add_currency<Token>(account: &signer) {
        move_to(account, Balance<Token>{ coin: Coin::zero<Token>() })
    }

    // Return whether the account at `addr` accepts `Token` type coins
    public fun accepts_currency<Token>(addr: address): bool {
        ::exists<Balance<Token>>(addr)
    }

    // Helper to return the sequence number field for given `account`
    fun sequence_number_for_account(account: &T): u64 {
        account.sequence_number
    }

    // Return the current sequence number at `addr`
    public fun sequence_number(addr: address): u64 acquires T {
        sequence_number_for_account(borrow_global<T>(addr))
    }

    // Return the authentication key for this account
    public fun authentication_key(addr: address): vector<u8> acquires T {
        *&borrow_global<T>(addr).authentication_key
    }

    // Return true if the account at `addr` has delegated its key rotation capability
    public fun delegated_key_rotation_capability(addr: address): bool acquires T {
        borrow_global<T>(addr).delegated_key_rotation_capability
    }

    // Return true if the account at `addr` has delegated its withdrawal capability
    public fun delegated_withdrawal_capability(addr: address): bool acquires T {
        borrow_global<T>(addr).delegated_withdrawal_capability
    }

    // Return a reference to the address associated with the given withdrawal capability
    public fun withdrawal_capability_address(cap: &WithdrawalCapability): &address {
        &cap.account_address
    }

    // Return a reference to the address associated with the given key rotation capability
    public fun key_rotation_capability_address(cap: &KeyRotationCapability): &address {
        &cap.account_address
    }

    // Checks if an account exists at `check_addr`
    public fun exists(check_addr: address): bool {
        ::exists<T>(check_addr)
    }

    ///////////////////////////////////////////////////////////////////////////
    // Freezing
    ///////////////////////////////////////////////////////////////////////////

    // Freeze the account at `addr`.
    public fun freeze_account(account: &signer, addr: address)
    acquires T {
        assert_can_freeze(Signer::address_of(account));
        // The root association account cannot be frozen
        Transaction::assert(addr != Association::root_address(), 14);
        borrow_global_mut<T>(addr).is_frozen = true;
    }

    // Unfreeze the account at `addr`.
    public fun unfreeze_account(account: &signer, addr: address)
    acquires T {
        assert_can_freeze(Signer::address_of(account));
        borrow_global_mut<T>(addr).is_frozen = false;
    }

    // Returns if the account at `addr` is frozen.
    public fun account_is_frozen(addr: address): bool
    acquires T {
        borrow_global<T>(addr).is_frozen
     }

    fun assert_can_freeze(addr: address) {
        Transaction::assert(Association::has_privilege<FreezingPrivilege>(addr), 13);
    }

    // The prologue is invoked at the beginning of every transaction
    // It verifies:
    // - The account's auth key matches the transaction's public key
    // - That the account has enough balance to pay for all of the gas
    // - That the sequence number matches the transaction's sequence key
    fun prologue<Token>(
        account: &signer,
        txn_sequence_number: u64,
        txn_public_key: vector<u8>,
        txn_gas_price: u64,
        txn_max_gas_units: u64,
        txn_expiration_time: u64,
    ) acquires T, Balance {
        let transaction_sender = Signer::address_of(account);

        // FUTURE: Make these error codes sequential
        // Verify that the transaction sender's account exists
        Transaction::assert(exists(transaction_sender), 5);

        Transaction::assert(!account_is_frozen(transaction_sender), 0);

        // Load the transaction sender's account
        let sender_account = borrow_global_mut<T>(transaction_sender);

        // Check that the hash of the transaction's public key matches the account's auth key
        Transaction::assert(
            Hash::sha3_256(txn_public_key) == *&sender_account.authentication_key,
            2
        );

        // Check that the account has enough balance for all of the gas
        let max_transaction_fee = txn_gas_price * txn_max_gas_units;
        let balance_amount = balance<Token>(transaction_sender);
        Transaction::assert(balance_amount >= max_transaction_fee, 6);

        // Check that the transaction sequence number matches the sequence number of the account
        Transaction::assert(txn_sequence_number >= sender_account.sequence_number, 3);
        Transaction::assert(txn_sequence_number == sender_account.sequence_number, 4);
        Transaction::assert(TransactionTimeout::is_valid_transaction_timestamp(txn_expiration_time), 7);
    }

    // The epilogue is invoked at the end of transactions.
    // It collects gas and bumps the sequence number
    fun epilogue<Token>(
        account: &signer,
        txn_sequence_number: u64,
        txn_gas_price: u64,
        txn_max_gas_units: u64,
        gas_units_remaining: u64
    ) acquires T, Balance {
        // Load the transaction sender's account and balance resources
        let sender_account = borrow_global_mut<T>(Signer::address_of(account));
        let sender_balance = borrow_global_mut<Balance<Token>>(Signer::address_of(account));

        // Charge for gas
        let transaction_fee_amount = txn_gas_price * (txn_max_gas_units - gas_units_remaining);
        Transaction::assert(
            balance_for(sender_balance) >= transaction_fee_amount,
            6
        );
        // Bump the sequence number
        sender_account.sequence_number = txn_sequence_number + 1;

        if (transaction_fee_amount > 0) {
            let transaction_fee = withdraw_from_balance(
                    Signer::address_of(account),
                    sender_balance,
                    transaction_fee_amount
            );
            // Pay the transaction fee into the transaction fee balance.
            // Don't use the account deposit in order to not emit a
            // sent/received payment event.
            let transaction_fee_balance = borrow_global_mut<Balance<Token>>(0xFEE);
            Coin::deposit(&mut transaction_fee_balance.coin, transaction_fee);
        }
    }
}

}
