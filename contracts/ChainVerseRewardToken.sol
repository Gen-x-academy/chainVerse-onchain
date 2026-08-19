// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";

contract ChainVerseRewardToken is ERC20, AccessControl {
    bytes32 public constant REWARDER_ROLE = keccak256("REWARDER_ROLE");
    uint256 public immutable CAP;

    error CapExceeded(uint256 requestedSupply, uint256 cap);
    error InvalidCap();

    constructor(address initialAdmin, uint256 tokenCap) ERC20("ChainVerse Reward", "CVR") {
        if (initialAdmin == address(0) || tokenCap == 0) revert InvalidCap();
        CAP = tokenCap;
        _grantRole(DEFAULT_ADMIN_ROLE, initialAdmin);
    }

    function reward(address student, uint256 amount) external onlyRole(REWARDER_ROLE) {
        uint256 requestedSupply = totalSupply() + amount;
        if (requestedSupply > CAP) revert CapExceeded(requestedSupply, CAP);
        _mint(student, amount);
    }
}
