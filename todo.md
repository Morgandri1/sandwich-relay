# ToDo
- [x] Identify swap platform from tx
- [ ] Find token mint from target tx
- [ ] get a fully serialized VersionedTx we can dissect and feed into deserializers until we get a useful result
- [ ] figure out how to deserialize tx.instruction.data into InstructionType variants
- [ ] detect jito tx bundles
- [ ] check if tx is part of bundle, and if so, check for a jito tip (transfer instruction to set list of accounts), and hijack bundle to include mev tx
- [ ] Copy all accounts from target tx except for token account (maybe clone static accounts -> derive token account address of original signer -> find and replace derived address in accounts vec -> resign copied buy tx -> use same accounts list for sell builder (?))
- [ ] Assemble swap with only input/output amounts, mint address, signer, and swap provider
- [ ] Calculate buy/sell amount from slippage (or lack thereof) of target transaction