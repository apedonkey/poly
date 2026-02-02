// Verify compute_position_id with FRESH market data from Gamma API
const { utils, BigNumber } = require('ethers');

// XRP market from API (negRisk: false)
const conditionId = "0xf2bc5c88f5e205286e06d4806923dc2761dd0df117110608ec0f95d9d182efba";
const collateral = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";

// CLOB token IDs from API
const expectedYes = "79271933354357980253136288605181231275403247125972443314258298490063254253319";
const expectedNo = "115773802315123866833219318493352139995639773046516640218056997998800039154013";

// Method: solidityKeccak256 (matches abi.encodePacked + keccak256)
// YES (indexSet=1):
const collYes = utils.solidityKeccak256(["bytes32", "uint256"], [conditionId, 1]);
const posYes = utils.solidityKeccak256(["address", "bytes32"], [collateral, collYes]);
console.log("YES computed:", BigNumber.from(posYes).toString());
console.log("YES expected:", expectedYes);
console.log("YES match:", BigNumber.from(posYes).toString() === expectedYes);

// NO (indexSet=2):
const collNo = utils.solidityKeccak256(["bytes32", "uint256"], [conditionId, 2]);
const posNo = utils.solidityKeccak256(["address", "bytes32"], [collateral, collNo]);
console.log("\nNO computed:", BigNumber.from(posNo).toString());
console.log("NO expected:", expectedNo);
console.log("NO match:", BigNumber.from(posNo).toString() === expectedNo);

// Also try the BTC condition from the current run: 0x4bf45ae9
// (We don't have the full ID, so let's try with the SOL market too)
const solCondition = "0x619fae6692cb7f280705d0c91b6676f0110835c8bc1abba04c09ed3a456edcaa";
const solYesExpected = "30591171428592617970483248169804219463157682264513959443927731429695640682039";

const solCollYes = utils.solidityKeccak256(["bytes32", "uint256"], [solCondition, 1]);
const solPosYes = utils.solidityKeccak256(["address", "bytes32"], [collateral, solCollYes]);
console.log("\nSOL YES computed:", BigNumber.from(solPosYes).toString());
console.log("SOL YES expected:", solYesExpected);
console.log("SOL YES match:", BigNumber.from(solPosYes).toString() === solYesExpected);
