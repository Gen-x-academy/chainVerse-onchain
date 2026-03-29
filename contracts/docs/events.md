# Contract Events

The system emits various events useful for indexing. This documents standard event structures emitted. All Events follow a `chainverse:{domain}:{event_name}` format.

## Standard Events

### CoursePurchased

- **Topic**: `chainverse:course:purchased`
- **Description**: Emitted when a user successfully buys a registered course and deposits funds into escrow.
- **Payload**:
  ```rust
  (buyer: Address, course_id: u64, amount: i128)
  ```

### RewardClaimed

- **Topic**: `chainverse:reward:claimed`
- **Description**: Emitted after a user earns or claims their allocated token rewards for completing the course.
- **Payload**:
  ```rust
  (user: Address, reward_id: u64, amount: i128)
  ```

### CertificateMinted

- **Topic**: `chainverse:certificate:minted`
- **Description**: Emitted once a non-fungible certificate token is successfully minted and awarded to the user upon course completion.
- **Payload**:
  ```rust
  (user: Address, course_id: u64, token_id: u64)
  ```

### EscrowReleased

- **Topic**: `chainverse:escrow:released`
- **Description**: Emitted when the escrow has authorized and transferred the locked funds to the course creator.
- **Payload**:
  ```rust
  (course_id: u64, creator: Address, amount: i128)
  ```

### EscrowRefunded

- **Topic**: `chainverse:escrow:refunded`
- **Description**: Emitted when funds are returned successfully to the user's wallet due to a fallback condition.
- **Payload**:
  ```rust
  (course_id: u64, user: Address, amount: i128)
  ```
