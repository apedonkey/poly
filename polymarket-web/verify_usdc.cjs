const { utils, BigNumber } = require('ethers');

const conditionId = "0xf2bc5c88f5e205286e06d4806923dc2761dd0df117110608ec0f95d9d182efba";

// Try both USDC addresses
const usdc_bridged = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"; // USDC.e
const usdc_native = "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359";  // Native USDC

const expectedYes = "79271933354357980253136288605181231275403247125972443314258298490063254253319";

// With bridged USDC
const coll1 = utils.solidityKeccak256(["bytes32", "uint256"], [conditionId, 1]);
const pos1_bridged = utils.solidityKeccak256(["address", "bytes32"], [usdc_bridged, coll1]);
console.log("Bridged USDC:", BigNumber.from(pos1_bridged).toString());
console.log("Match:", BigNumber.from(pos1_bridged).toString() === expectedYes);

// With native USDC
const pos1_native = utils.solidityKeccak256(["address", "bytes32"], [usdc_native, coll1]);
console.log("Native USDC:", BigNumber.from(pos1_native).toString());
console.log("Match:", BigNumber.from(pos1_native).toString() === expectedYes);

// What if Polymarket wraps USDC and uses WrappedCollateral even for non-NegRisk?
// The WrappedCollateral address might be something else
// Let's try zero address
const pos1_zero = utils.solidityKeccak256(["address", "bytes32"], ["0x0000000000000000000000000000000000000000", coll1]);
console.log("Zero address:", BigNumber.from(pos1_zero).toString());
console.log("Match:", BigNumber.from(pos1_zero).toString() === expectedYes);

// What if the collectionId uses a parentCollectionId even for non-NegRisk?
// Try various parentCollectionId values
console.log("\n--- Trying different parentCollectionId values ---");

// What if parentCollectionId = conditionId itself?
const hash1 = BigNumber.from(utils.solidityKeccak256(["bytes32", "uint256"], [conditionId, 1]));
const parent1 = BigNumber.from(conditionId);
const coll1_with_parent = "0x" + hash1.add(parent1).toHexString().slice(2).padStart(64, "0");
const pos1_with_parent = utils.solidityKeccak256(["address", "bytes32"], [usdc_bridged, coll1_with_parent]);
console.log("With conditionId as parent:", BigNumber.from(pos1_with_parent).toString());
console.log("Match:", BigNumber.from(pos1_with_parent).toString() === expectedYes);

// What about the polymarket indexSet ordering?
// Maybe YES is indexSet=0 and NO is indexSet=1?
// No, that doesn't make sense - indexSets are bitmasks, minimum is 1

// What if the contract uses abi.encode instead of abi.encodePacked for getCollectionId?
const coll1_encode = utils.keccak256(utils.defaultAbiCoder.encode(["bytes32", "uint256"], [conditionId, 1]));
const pos1_encode = utils.solidityKeccak256(["address", "bytes32"], [usdc_bridged, coll1_encode]);
console.log("\nWith abi.encode (not packed) for collection:", BigNumber.from(pos1_encode).toString());
console.log("Match:", BigNumber.from(pos1_encode).toString() === expectedYes);

// What if BOTH use abi.encode?
const pos1_all_encode = utils.keccak256(utils.defaultAbiCoder.encode(["address", "bytes32"], [usdc_bridged, coll1_encode]));
console.log("Both abi.encode:", BigNumber.from(pos1_all_encode).toString());
console.log("Match:", BigNumber.from(pos1_all_encode).toString() === expectedYes);

// What if getPositionId also uses abi.encode?
const pos1_encode2 = utils.keccak256(utils.defaultAbiCoder.encode(["address", "bytes32"], [usdc_bridged, coll1]));
console.log("Position with abi.encode, collection with packed:", BigNumber.from(pos1_encode2).toString());
console.log("Match:", BigNumber.from(pos1_encode2).toString() === expectedYes);

// What if they swap YES/NO? Index 1 = NO, Index 2 = YES?
const coll2 = utils.solidityKeccak256(["bytes32", "uint256"], [conditionId, 2]);
const pos2_bridged = utils.solidityKeccak256(["address", "bytes32"], [usdc_bridged, coll2]);
console.log("\nSwapped? indexSet=2 with bridged USDC:", BigNumber.from(pos2_bridged).toString());
console.log("Match YES:", BigNumber.from(pos2_bridged).toString() === expectedYes);
