const { expect } = require("chai");
const { ethers } = require("hardhat");

describe("ChainVerse MVP", function () {
  async function deployFixture() {
    const [admin, instructor, student, recipient, treasury] = await ethers.getSigners();
    const registry = await ethers.deployContract("ChainVerseCourseRegistry", [admin.address]);
    const certificate = await ethers.deployContract("ChainVerseCertificate", [admin.address]);
    const reward = await ethers.deployContract("ChainVerseRewardToken", [
      admin.address,
      ethers.parseEther("1000000"),
    ]);
    const marketplace = await ethers.deployContract("ChainVerseMarketplace", [
      admin.address,
      await registry.getAddress(),
      await certificate.getAddress(),
      await reward.getAddress(),
      treasury.address,
      ethers.parseEther("10"),
    ]);

    await certificate.grantRole(await certificate.MINTER_ROLE(), await marketplace.getAddress());
    await reward.grantRole(await reward.REWARDER_ROLE(), await marketplace.getAddress());

    return { admin, instructor, student, recipient, treasury, registry, certificate, reward, marketplace };
  }

  async function createActiveCourse(ctx) {
    const price = ethers.parseEther("0.1");
    await ctx.registry.connect(ctx.instructor).createCourse(price, 500, "ipfs://course");
    await ctx.registry.setCourseApproval(1, true);
    await ctx.registry.connect(ctx.instructor).updateCourse(1, price, true, "ipfs://course");
    return price;
  }

  it("splits a purchase into instructor and platform balances", async function () {
    const ctx = await deployFixture();
    const price = await createActiveCourse(ctx);

    await expect(ctx.marketplace.connect(ctx.student).purchaseCourse(1, { value: price }))
      .to.emit(ctx.marketplace, "CoursePurchased")
      .withArgs(1, ctx.student.address, ctx.instructor.address, price, price / 20n);

    expect(await ctx.marketplace.isEnrolled(1, ctx.student.address)).to.equal(true);
    expect(await ctx.marketplace.instructorEarnings(ctx.instructor.address)).to.equal(price * 95n / 100n);
    expect(await ctx.marketplace.platformEarnings()).to.equal(price / 20n);
  });

  it("allows an unfinished enrollment to be gifted exactly once", async function () {
    const ctx = await deployFixture();
    const price = await createActiveCourse(ctx);
    await ctx.marketplace.connect(ctx.student).purchaseCourse(1, { value: price });

    await ctx.marketplace.connect(ctx.student).transferEnrollment(1, ctx.recipient.address);

    expect(await ctx.marketplace.isEnrolled(1, ctx.student.address)).to.equal(false);
    expect(await ctx.marketplace.isEnrolled(1, ctx.recipient.address)).to.equal(true);
  });

  it("issues a soulbound certificate and reward after verified completion", async function () {
    const ctx = await deployFixture();
    const price = await createActiveCourse(ctx);
    await ctx.marketplace.connect(ctx.student).purchaseCourse(1, { value: price });

    await ctx.marketplace.completeCourse(1, ctx.student.address, "ipfs://certificate");

    expect(await ctx.certificate.certificateOf(ctx.student.address, 1)).to.equal(1);
    expect(await ctx.reward.balanceOf(ctx.student.address)).to.equal(ethers.parseEther("10"));
    await expect(
      ctx.certificate.connect(ctx.student).transferFrom(ctx.student.address, ctx.recipient.address, 1),
    ).to.be.revertedWithCustomError(ctx.certificate, "SoulboundCertificate");
  });

  it("rejects duplicate enrollment and duplicate completion", async function () {
    const ctx = await deployFixture();
    const price = await createActiveCourse(ctx);
    await ctx.marketplace.connect(ctx.student).purchaseCourse(1, { value: price });

    await expect(ctx.marketplace.connect(ctx.student).purchaseCourse(1, { value: price }))
      .to.be.revertedWithCustomError(ctx.marketplace, "AlreadyEnrolled");

    await ctx.marketplace.completeCourse(1, ctx.student.address, "ipfs://certificate");
    await expect(ctx.marketplace.completeCourse(1, ctx.student.address, "ipfs://certificate"))
      .to.be.revertedWithCustomError(ctx.marketplace, "AlreadyCompleted");
  });
});
