import type { AccountMeta, ReadonlyUint8Array } from '@solana/kit';

type MessageAccountsResolverScope = Readonly<{
    args: Readonly<{ message: ReadonlyUint8Array }>;
}>;

type TransactionAccountsResolverScope = Readonly<{
    args: Readonly<{ transaction: ReadonlyUint8Array }>;
}>;

// placeholder for next PR
export const resolveMessageAccounts = (_scope: MessageAccountsResolverScope): AccountMeta[] => {
    throw new Error('resolveMessageAccounts is not implemented');
};

// placeholder for next PR
export const resolveTransactionAccounts = (_scope: TransactionAccountsResolverScope): AccountMeta[] => {
    throw new Error('resolveTransactionAccounts is not implemented');
};
