---
feature: Deposit Money
version: 1.0
doc: functions/deposit, functions/getBalance, classes/BankAccount
---

## Overview

Allows depositing an amount into a bank account and querying the resulting balance.

## Flow

**Given** a BankAccount with an existing balance  
**When** `deposit` is called with a positive amount  
**Then** the account balance increases by that amount  
**And** `getBalance` returns the updated value

## Input / Output

| Input | Type | Description |
|-------|------|-------------|
| account | BankAccount | The target account |
| amount | number | Amount to deposit (must be > 0) |

| Output | Type | Description |
|--------|------|-------------|
| — | void | Balance updated in place |

## Error States

- Depositing a negative amount produces undefined behavior (no guard defined yet).
