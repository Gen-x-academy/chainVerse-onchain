// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {AccessControl} from "@openzeppelin/contracts/access/AccessControl.sol";
import {ERC721} from "@openzeppelin/contracts/token/ERC721/ERC721.sol";
import {ERC721URIStorage} from "@openzeppelin/contracts/token/ERC721/extensions/ERC721URIStorage.sol";

contract ChainVerseCertificate is ERC721URIStorage, AccessControl {
    bytes32 public constant MINTER_ROLE = keccak256("MINTER_ROLE");

    uint256 private _nextTokenId = 1;
    mapping(address student => mapping(uint256 courseId => uint256 tokenId)) public certificateOf;

    error CertificateAlreadyIssued(address student, uint256 courseId);
    error SoulboundCertificate();
    error InvalidRecipient();

    event CertificateIssued(
        uint256 indexed tokenId,
        uint256 indexed courseId,
        address indexed student,
        string metadataURI
    );

    constructor(address initialAdmin) ERC721("ChainVerse Certificate", "CVCERT") {
        if (initialAdmin == address(0)) revert InvalidRecipient();
        _grantRole(DEFAULT_ADMIN_ROLE, initialAdmin);
    }

    function issueCertificate(
        address student,
        uint256 courseId,
        string calldata metadataURI
    ) external onlyRole(MINTER_ROLE) returns (uint256 tokenId) {
        if (student == address(0)) revert InvalidRecipient();
        if (certificateOf[student][courseId] != 0) {
            revert CertificateAlreadyIssued(student, courseId);
        }

        tokenId = _nextTokenId++;
        certificateOf[student][courseId] = tokenId;
        _safeMint(student, tokenId);
        _setTokenURI(tokenId, metadataURI);
        emit CertificateIssued(tokenId, courseId, student, metadataURI);
    }

    function _update(address to, uint256 tokenId, address auth)
        internal
        override
        returns (address from)
    {
        from = _ownerOf(tokenId);
        if (from != address(0) && to != address(0)) revert SoulboundCertificate();
        return super._update(to, tokenId, auth);
    }

    function supportsInterface(bytes4 interfaceId)
        public
        view
        override(ERC721URIStorage, AccessControl)
        returns (bool)
    {
        return super.supportsInterface(interfaceId);
    }
}
