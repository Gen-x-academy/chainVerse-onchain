// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {IChainVerseCourseRegistry} from "./interfaces/IChainVerseCourseRegistry.sol";
import {ChainVerseCertificate} from "./ChainVerseCertificate.sol";
import {ChainVerseRewardToken} from "./ChainVerseRewardToken.sol";

contract ChainVerseMarketplace is AccessControl, ReentrancyGuard {
    bytes32 public constant COMPLETION_VERIFIER_ROLE = keccak256("COMPLETION_VERIFIER_ROLE");
    bytes32 public constant TREASURY_MANAGER_ROLE = keccak256("TREASURY_MANAGER_ROLE");

    IChainVerseCourseRegistry public immutable COURSE_REGISTRY;
    ChainVerseCertificate public immutable CERTIFICATE;
    ChainVerseRewardToken public immutable REWARD_TOKEN;

    address payable public platformTreasury;
    uint256 public completionReward;

    mapping(uint256 courseId => mapping(address student => bool enrolled)) public isEnrolled;
    mapping(uint256 courseId => mapping(address student => bool completed)) public hasCompleted;
    mapping(address instructor => uint256 balance) public instructorEarnings;
    uint256 public platformEarnings;

    error InvalidAddress();
    error InvalidPayment(uint256 expected, uint256 received);
    error CourseUnavailable(uint256 courseId);
    error AlreadyEnrolled(address student, uint256 courseId);
    error NotEnrolled(address student, uint256 courseId);
    error AlreadyCompleted(address student, uint256 courseId);
    error TransferToSelf();
    error WithdrawalFailed();
    error NothingToWithdraw();

    event CoursePurchased(
        uint256 indexed courseId,
        address indexed student,
        address indexed instructor,
        uint256 price,
        uint256 platformFee
    );
    event EnrollmentTransferred(
        uint256 indexed courseId,
        address indexed from,
        address indexed to
    );
    event CourseCompleted(uint256 indexed courseId, address indexed student, uint256 reward);
    event InstructorWithdrawal(address indexed instructor, uint256 amount);
    event PlatformWithdrawal(address indexed treasury, uint256 amount);

    constructor(
        address initialAdmin,
        IChainVerseCourseRegistry registry,
        ChainVerseCertificate certificateContract,
        ChainVerseRewardToken rewardContract,
        address payable treasury,
        uint256 initialCompletionReward
    ) {
        if (
            initialAdmin == address(0) || address(registry) == address(0)
                || address(certificateContract) == address(0) || address(rewardContract) == address(0)
                || treasury == address(0)
        ) revert InvalidAddress();

        COURSE_REGISTRY = registry;
        CERTIFICATE = certificateContract;
        REWARD_TOKEN = rewardContract;
        platformTreasury = treasury;
        completionReward = initialCompletionReward;

        _grantRole(DEFAULT_ADMIN_ROLE, initialAdmin);
        _grantRole(COMPLETION_VERIFIER_ROLE, initialAdmin);
        _grantRole(TREASURY_MANAGER_ROLE, initialAdmin);
    }

    function purchaseCourse(uint256 courseId) external payable {
        IChainVerseCourseRegistry.Course memory course = COURSE_REGISTRY.getCourse(courseId);
        if (!course.approved || !course.active) revert CourseUnavailable(courseId);
        if (isEnrolled[courseId][msg.sender]) revert AlreadyEnrolled(msg.sender, courseId);
        if (msg.value != course.price) revert InvalidPayment(course.price, msg.value);

        uint256 platformFee = (msg.value * course.platformFeeBps) / 10_000;
        instructorEarnings[course.instructor] += msg.value - platformFee;
        platformEarnings += platformFee;
        isEnrolled[courseId][msg.sender] = true;

        emit CoursePurchased(courseId, msg.sender, course.instructor, msg.value, platformFee);
    }

    function transferEnrollment(uint256 courseId, address recipient) external {
        if (recipient == address(0)) revert InvalidAddress();
        if (recipient == msg.sender) revert TransferToSelf();
        if (!isEnrolled[courseId][msg.sender]) revert NotEnrolled(msg.sender, courseId);
        if (hasCompleted[courseId][msg.sender]) revert AlreadyCompleted(msg.sender, courseId);
        if (isEnrolled[courseId][recipient]) revert AlreadyEnrolled(recipient, courseId);

        isEnrolled[courseId][msg.sender] = false;
        isEnrolled[courseId][recipient] = true;
        emit EnrollmentTransferred(courseId, msg.sender, recipient);
    }

    function completeCourse(
        uint256 courseId,
        address student,
        string calldata certificateURI
    ) external onlyRole(COMPLETION_VERIFIER_ROLE) {
        if (!isEnrolled[courseId][student]) revert NotEnrolled(student, courseId);
        if (hasCompleted[courseId][student]) revert AlreadyCompleted(student, courseId);

        hasCompleted[courseId][student] = true;
        CERTIFICATE.issueCertificate(student, courseId, certificateURI);
        if (completionReward != 0) REWARD_TOKEN.reward(student, completionReward);
        emit CourseCompleted(courseId, student, completionReward);
    }

    function setCompletionReward(uint256 newReward) external onlyRole(DEFAULT_ADMIN_ROLE) {
        completionReward = newReward;
    }

    function setPlatformTreasury(address payable newTreasury)
        external
        onlyRole(DEFAULT_ADMIN_ROLE)
    {
        if (newTreasury == address(0)) revert InvalidAddress();
        platformTreasury = newTreasury;
    }

    function withdrawInstructorEarnings() external nonReentrant {
        uint256 amount = instructorEarnings[msg.sender];
        if (amount == 0) revert NothingToWithdraw();
        instructorEarnings[msg.sender] = 0;
        _sendValue(payable(msg.sender), amount);
        emit InstructorWithdrawal(msg.sender, amount);
    }

    function withdrawPlatformEarnings() external onlyRole(TREASURY_MANAGER_ROLE) nonReentrant {
        uint256 amount = platformEarnings;
        if (amount == 0) revert NothingToWithdraw();
        platformEarnings = 0;
        _sendValue(platformTreasury, amount);
        emit PlatformWithdrawal(platformTreasury, amount);
    }

    function _sendValue(address payable recipient, uint256 amount) private {
        (bool success,) = recipient.call{value: amount}("");
        if (!success) revert WithdrawalFailed();
    }
}
