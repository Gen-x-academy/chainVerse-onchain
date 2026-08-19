const { ethers } = require("hardhat");

async function main() {
  const [deployer] = await ethers.getSigners();
  const treasury = process.env.PLATFORM_TREASURY_ADDRESS || deployer.address;

  const registry = await ethers.deployContract("ChainVerseCourseRegistry", [deployer.address]);
  await registry.waitForDeployment();

  const certificate = await ethers.deployContract("ChainVerseCertificate", [deployer.address]);
  await certificate.waitForDeployment();

  const rewardCap = ethers.parseEther("100000000");
  const reward = await ethers.deployContract("ChainVerseRewardToken", [deployer.address, rewardCap]);
  await reward.waitForDeployment();

  const marketplace = await ethers.deployContract("ChainVerseMarketplace", [
    deployer.address,
    await registry.getAddress(),
    await certificate.getAddress(),
    await reward.getAddress(),
    treasury,
    ethers.parseEther("10"),
  ]);
  await marketplace.waitForDeployment();

  await (await certificate.grantRole(await certificate.MINTER_ROLE(), await marketplace.getAddress())).wait();
  await (await reward.grantRole(await reward.REWARDER_ROLE(), await marketplace.getAddress())).wait();

  console.log({
    deployer: deployer.address,
    treasury,
    registry: await registry.getAddress(),
    certificate: await certificate.getAddress(),
    reward: await reward.getAddress(),
    marketplace: await marketplace.getAddress(),
  });
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
