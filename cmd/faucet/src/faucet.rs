use anyhow::{format_err, Result};

use starcoin_rpc_client::{RemoteStateReader, RpcClient};
use starcoin_state_api::AccountStateReader;
use starcoin_types::account_address::AccountAddress;
use starcoin_wallet_api::WalletAccount;

pub struct Faucet {
    client: RpcClient,
    faucet_account: WalletAccount,
}

const DEFAULT_GAS_PRICE: u64 = 1;
const MAX_GAS: u64 = 50_000_000;

impl Faucet {
    pub fn new(client: RpcClient, faucet_account: WalletAccount) -> Self {
        Faucet {
            client,
            faucet_account,
        }
    }

    pub fn transfer(
        &self,
        amount: u64,
        receiver: AccountAddress,
        auth_key: Vec<u8>,
    ) -> Result<Result<(), anyhow::Error>> {
        let chain_state_reader = RemoteStateReader::new(&self.client);
        let account_state_reader = AccountStateReader::new(&chain_state_reader);
        let account_resource = account_state_reader
            .get_account_resource(self.faucet_account.address())?
            .ok_or_else(|| {
                format_err!(
                    "Can not find account on chain by address:{}",
                    self.faucet_account.address()
                )
            })?;

        let raw_tx = starcoin_executor::build_transfer_txn(
            self.faucet_account.address,
            receiver,
            auth_key,
            account_resource.sequence_number(),
            amount,
            DEFAULT_GAS_PRICE,
            MAX_GAS,
        );
        let signed_tx = self.client.wallet_sign_txn(raw_tx)?;
        let ret = self.client.submit_transaction(signed_tx)?;
        Ok(ret)
    }
}
