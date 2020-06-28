//! account: alice

//! sender: alice
module Test {

    resource struct Coin {
        value: u8,
    }

    public fun new(account: &signer, value: u8) {
        let coin = Coin {value};
        move_to(account, coin);
    }

    public fun value(account: address): u8 acquires Coin {
        let coin_ref = borrow_global<Self::Coin>(account);
        *&coin_ref.value
    } 
}

// check: EXECUTED

//! new-transaction
//! sender: alice
script {
use {{alice}}::Test;
use 0x1::Signer;
fun main(account: &signer) {
    let sender = Signer::address_of(account);
    Test::new(account, 100);

    // move_to(account, coin);
    // let _ = borrow_global<Test::Coin>(sender);
    assert(Test::value(sender) == 100, 21);
}
}

// check: EXECUTED
