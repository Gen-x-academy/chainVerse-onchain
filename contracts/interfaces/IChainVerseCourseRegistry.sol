// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

interface IChainVerseCourseRegistry {
    struct Course {
        address instructor;
        uint96 price;
        uint16 platformFeeBps;
        bool approved;
        bool active;
        string metadataURI;
    }

    function getCourse(uint256 courseId) external view returns (Course memory);
}
