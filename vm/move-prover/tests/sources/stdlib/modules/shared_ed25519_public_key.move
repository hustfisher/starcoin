// Each address that holds a `SharedEd25519PublicKey` resource can rotate the public key stored in
// this resource, but the account's authentication key will be updated in lockstep. This ensures
// that the two keys always stay in sync.

address 0x0 {
module SharedEd25519PublicKey {
    use 0x0::Authenticator;
    use 0x0::Account;
    use 0x0::Signature;
    use 0x0::Signer;
    use 0x0::Transaction;

    spec module {
        pragma verify = false;
    }

    // A resource that forces the account associated with `rotation_cap` to use a ed25519
    // authentication key derived from `key`
    resource struct T {
        // 32 byte ed25519 public key
        key: vector<u8>,
        // rotation capability for an account whose authentication key is always derived from `key`
        rotation_cap: Account::KeyRotationCapability,
    }

    // (1) Rotate the authentication key of the sender to `key`
    // (2) Publish a resource containing a 32-byte ed25519 public key and the rotation capability
    //     of the sender under the `account`'s address.
    // Aborts if the sender already has a `SharedEd25519PublicKey` resource.
    // Aborts if the length of `new_public_key` is not 32.
    public fun publish(account: &signer, key: vector<u8>) {
        let t = T {
            key: x"",
            rotation_cap: Account::extract_sender_key_rotation_capability(account)
        };
        rotate_key_(&mut t, key);
        move_to(account, t);
    }

    fun rotate_key_(shared_key: &mut T, new_public_key: vector<u8>) {
        // Cryptographic check of public key validity
        Transaction::assert(
            Signature::ed25519_validate_pubkey(copy new_public_key),
            9003, // TODO: proper error code
        );
        Account::rotate_authentication_key_with_capability(
            &shared_key.rotation_cap,
            Authenticator::ed25519_authentication_key(copy new_public_key)
        );
        shared_key.key = new_public_key;
    }

    // (1) rotate the public key stored `account`'s `SharedEd25519PublicKey` resource to
    // `new_public_key`
    // (2) rotate the authentication key using the capability stored in the `account`'s
    // `SharedEd25519PublicKey` to a new value derived from `new_public_key`
    // Aborts if the sender does not have a `SharedEd25519PublicKey` resource.
    // Aborts if the length of `new_public_key` is not 32.
    public fun rotate_key(account: &signer, new_public_key: vector<u8>) acquires T {
        rotate_key_(borrow_global_mut<T>(Signer::address_of(account)), new_public_key);
    }

    // Return the public key stored under `addr`.
    // Aborts if `addr` does not hold a `SharedEd25519PublicKey` resource.
    public fun key(addr: address): vector<u8> acquires T {
        *&borrow_global<T>(addr).key
    }

    // Returns true if `addr` holds a `SharedEd25519PublicKey` resource.
    public fun exists(addr: address): bool {
        ::exists<T>(addr)
    }

}
}
