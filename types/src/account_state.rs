// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2.0

use libra_crypto::HashValue;

pub struct AccountState {
    code_root: HashValue,
    storage_root: HashValue,
}