use crate::helper::*;
use std::collections::HashMap;

use bincode;

use alloy_primitives::{address, Address, FixedBytes, Uint};
use alloy_sol_types::{sol, SolCall};
use reth_primitives::{keccak256, Header};
use sp1_cc_client_executor::{io::EVMStateSketch, ClientExecutor, ContractInput};

sol! {
    /// The public values encoded as a struct that can be easily deserialized inside Solidity.
    struct PublicValuesStruct {
        bytes32 bor_block_hash;
        bytes32 l1_block_hash;
    }
}

sol! {
    contract ConsensusProofVerifier {
        function verifyConsensusProof(bytes calldata _proofBytes, bytes32 bor_block_hash, bytes32 l1_block_hash) public view;
        function getEncodedValidatorInfo() public view returns(address[] memory, uint256[] memory, uint256);
    }
}

const VERIFIER_CONTRACT: Address = address!("1d42064Fc4Beb5F8aAF85F4617AE8b3b5B8Bd801");
const CALLER: Address = address!("0000000000000000000000000000000000000000");

#[derive(Clone)]
pub struct MilestoneProofInputs {
    pub tx_data: String,
    pub tx_hash: FixedBytes<32>,
    pub precommits: Vec<Vec<u8>>,
    pub sigs: Vec<String>,
    pub signers: Vec<Address>,
    pub bor_header: Header,
    pub bor_block_hash: FixedBytes<32>,
    pub state_sketch_bytes: Vec<u8>,
    pub l1_block_hash: FixedBytes<32>,
}

pub struct MilestoneProver {
    inputs: MilestoneProofInputs,
}

impl MilestoneProver {
    pub fn init(inputs: MilestoneProofInputs) -> Self {
        MilestoneProver { inputs }
    }

    pub fn prove(&self) {
        // Verify if the transaction data provided is actually correct or not
        let milestone = verify_tx_data(&self.inputs.tx_data, &self.inputs.tx_hash);

        // Verify if the bor block header matches with the milestone or not
        assert_eq!(
            milestone.end_block, self.inputs.bor_header.number,
            "block number mismatch between milestone and bor block header"
        );
        assert_eq!(
            milestone.hash,
            self.inputs.bor_header.hash_slow().to_vec(),
            "block hash mismatch between milestone and bor block header"
        );

        // Make sure that we have equal number of precommits, signatures and signers.
        assert_eq!(self.inputs.precommits.len(), self.inputs.sigs.len());
        assert_eq!(self.inputs.sigs.len(), self.inputs.signers.len());

        let state_sketch =
            bincode::deserialize::<EVMStateSketch>(&self.inputs.state_sketch_bytes).unwrap();

        // Initialize the client executor with the state sketch.
        // This step also validates all of the storage against the provided state root.
        let executor = ClientExecutor::new(state_sketch).unwrap();

        // Execute the `getEncodedValidatorInfo` call using the client executor to fetch the
        // active validator's info from L1.
        let call = ConsensusProofVerifier::getEncodedValidatorInfoCall {};
        let input = ContractInput {
            contract_address: VERIFIER_CONTRACT,
            caller_address: CALLER,
            calldata: call.clone(),
        };
        let output = executor.execute(input).unwrap();
        let response = ConsensusProofVerifier::getEncodedValidatorInfoCall::abi_decode_returns(
            &output.contractOutput,
            true,
        )
        .unwrap();

        // Extract the signers, powers, and total_power from the response.
        let signers = response._0;
        let powers = response._1;
        let total_power = response._2;

        let majority_power: Uint<256, 4> = Uint::from(0);
        let mut validator_stakes = HashMap::new();
        for (i, signer) in signers.iter().enumerate() {
            validator_stakes.insert(signer, powers[i]);
        }

        // Verify that the signatures generated by signing the precommit message are indeed signed
        // by the given validators.
        for i in 0..self.inputs.precommits.len() {
            // Validate if the signer of this precommit message is a part of the active validator
            // set or not.
            assert!(validator_stakes.contains_key(&self.inputs.signers[i]));

            // Verify if the precommit message is for the same milestone transaction or not.
            let precommit = &self.inputs.precommits[i];
            verify_precommit(&mut precommit.clone(), &self.inputs.tx_hash);

            // Verify if the message is indeed signed by the validator or not.
            verify_signature(
                self.inputs.sigs[i].as_str(),
                &keccak256(precommit),
                self.inputs.signers[i],
            );

            // Add the power of the validator to the majority power
            let _ = majority_power.add_mod(validator_stakes[&self.inputs.signers[i]], Uint::MAX);
        }

        // Check if the majority power is greater than 2/3rd of the total power
        let expected_majority = total_power
            .mul_mod(Uint::from(2), Uint::MAX)
            .div_ceil(Uint::from(3));
        if majority_power <= expected_majority {
            panic!("Majority voting power is less than 2/3rd of the total power");
        }
    }

    pub fn get_data_from_l1(&self) {}
}

// #[cfg(test)]
// mod tests {
//     use std::str::FromStr;

//     use super::*;
//     use alloy_primitives::FixedBytes;
//     use reth_primitives::{hex, hex::FromHex};

//     #[test]
//     fn test_valid_proof() {
//         let inputs = MilestoneProofInputs {
//         tx_data: "6AHwYl3uCp4B0ss+ZgoUqmrAL92q9vEg9buYzjCAnRnNXRsQ7+TrHBj75OscIiBVELbKUXyxwqqVdn4ZonVftpOEigC+o0PYMQp9wEQZbSoDMTM3MlEzNzNhY2I4Zi03OGVlLTRkMzctODYwZS03NWZkZjQyZjgyYTQgLSAweDE5YTI3NTVmYjY5Mzg0OGEwMGJlYTM0M2Q4MzEwYTdkYzA0NDE5NmQSQegVvhAXsyj7DXQUYSl+FoNvO/9cvdY2gGKIGtaDOgeSdXZyn5PrNKArkVTgndRHNC+17h4ZO9rF1TF9gEjOwvYB".to_string(),
//         tx_hash: FixedBytes::from_str(&"4C6BB9C1426CEF3B0252EFADFBD09B88350F508CC2A4EC0C837612958AD37C85").unwrap(),
//         precommits: vec!["9701080211327b30010000000022480a20fd648de965c020911f2bcfa3825fe2bd6698aa93009f0e63348ad74506221fae12240a20218d85717b5904942ce7c7b89b201aa1c2711dddb6e380cd0357c4647f35ac9b10012a0c08f29ee6b50610abfdeacc03320c6865696d64616c6c2d31333742240a204c6bb9c1426cef3b0252efadfbd09b88350f508cc2a4ec0c837612958ad37c851001".to_string()],
//         sigs: vec!["ZnLPE6+g9xfOQdnmugJ94zFRuQO47bH424V62XFgNul/HiiA46RBQYxW0E3+3MpcMLX4Dw5ma1rfFMr4Lr6EDwA=".to_string()],
//         signers: vec!["0x00856730088A5C3191BD26EB482E45229555CE57".to_string()],
//         headers: "f902d5a095913a64f4f93aeb8fd5ee7c6562bd02e50f4613a5ea230fbbf0aca717996217a01dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347940000000000000000000000000000000000000000a09eb7f80eafdb4f69520ec7d664402e5b3ce4af3b22cacc5098ebb73342b52301a01fa16db19c3fdc00a6902e29a57a1755dce6be40c4eab66e4b4a16d36a8e9135a00acd40f3f7dc9777f9e39a49654b4e564f18c22ca7ca025e69ea9790b47edd46b90100652a812e80ac124a006b29388ea4d67932562030482081304ea12036c01824200748231b1180241080620011510a9215241492384822a94320320e02aca0301a0608984c0e225aec2200248c103810f8522a2622415f00007843711092b69a0b13a040640b12b2602048e55a011e0d0816e40c505970044ce9a84310020b03f104b31980210f40992051c2c4f484c000180b16b339400b69505982ea42109205ea00238410d8440a906a101074014800f8c42546060068ce1410a600010560e343b1064334100210418b08302a68c003f6050004405aa0560292930b2085aa4534104247000454000c8d0842f520c5d2c01d816010c8094283d82a20231729341784039af27b8401c9c38083821b6d8466b98f45b8d7d78301030683626f7288676f312e32322e35856c696e75780000000000000000f87480f871c0c0c0c101c103c0c104c0c0c108c106c0c0c10ac0c10ec0c110c111c112c113c114c115c116c117c118c119c11ac11bc10fc0c0c10dc0c120c0c122c0c11dc0c124c0c0c20528c12bc0c12cc12ec0c12fc123c0c131c131c0c136c137c138c139c13ac13bc13cc13dc13ec13fc140c141b83e7c0cdd231a11d6cb176bf79f93231670ab808cb1ccd519402acbd7f213032cb3627a00cdf172db0b73d1565c597a8cf2d82c47a022ef0152186e46d05d2901a0000000000000000000000000000000000000000000000000000000000000000088000000000000000028".to_string(),
//         powers: vec![100],
//         total_power: 100,
//     };

//         let prover = MilestoneProver::init(inputs);
//         prover.prove();
//     }
// }
