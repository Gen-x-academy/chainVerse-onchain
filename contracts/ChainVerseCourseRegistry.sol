// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";
import {IChainVerseCourseRegistry} from "./interfaces/IChainVerseCourseRegistry.sol";

contract ChainVerseCourseRegistry is AccessControl, IChainVerseCourseRegistry {
    bytes32 public constant ADMIN_ROLE = keccak256("ADMIN_ROLE");
    uint16 public constant MAX_PLATFORM_FEE_BPS = 2_000;

    uint256 private _nextCourseId = 1;
    mapping(uint256 courseId => Course course) private _courses;

    error CourseNotFound(uint256 courseId);
    error InvalidInstructor();
    error InvalidPrice();
    error PlatformFeeTooHigh(uint16 feeBps);
    error NotCourseInstructor(address caller);
    error CourseNotApproved(uint256 courseId);

    event CourseCreated(
        uint256 indexed courseId,
        address indexed instructor,
        uint96 price,
        string metadataURI
    );
    event CourseApprovalChanged(uint256 indexed courseId, bool approved);
    event CourseUpdated(uint256 indexed courseId, uint96 price, bool active, string metadataURI);

    constructor(address initialAdmin) {
        if (initialAdmin == address(0)) revert InvalidInstructor();
        _grantRole(DEFAULT_ADMIN_ROLE, initialAdmin);
        _grantRole(ADMIN_ROLE, initialAdmin);
    }

    function createCourse(
        uint96 price,
        uint16 platformFeeBps,
        string calldata metadataURI
    ) external returns (uint256 courseId) {
        if (price == 0) revert InvalidPrice();
        if (platformFeeBps > MAX_PLATFORM_FEE_BPS) {
            revert PlatformFeeTooHigh(platformFeeBps);
        }

        courseId = _nextCourseId++;
        _courses[courseId] = Course({
            instructor: msg.sender,
            price: price,
            platformFeeBps: platformFeeBps,
            approved: false,
            active: false,
            metadataURI: metadataURI
        });

        emit CourseCreated(courseId, msg.sender, price, metadataURI);
    }

    function setCourseApproval(uint256 courseId, bool approved) external onlyRole(ADMIN_ROLE) {
        Course storage course = _requireCourse(courseId);
        course.approved = approved;
        if (!approved) course.active = false;
        emit CourseApprovalChanged(courseId, approved);
    }

    function updateCourse(
        uint256 courseId,
        uint96 price,
        bool active,
        string calldata metadataURI
    ) external {
        Course storage course = _requireCourse(courseId);
        if (msg.sender != course.instructor) revert NotCourseInstructor(msg.sender);
        if (price == 0) revert InvalidPrice();
        if (active && !course.approved) revert CourseNotApproved(courseId);

        course.price = price;
        course.active = active;
        course.metadataURI = metadataURI;
        emit CourseUpdated(courseId, price, active, metadataURI);
    }

    function getCourse(uint256 courseId) external view returns (Course memory) {
        Course memory course = _courses[courseId];
        if (course.instructor == address(0)) revert CourseNotFound(courseId);
        return course;
    }

    function _requireCourse(uint256 courseId) private view returns (Course storage course) {
        course = _courses[courseId];
        if (course.instructor == address(0)) revert CourseNotFound(courseId);
    }
}
