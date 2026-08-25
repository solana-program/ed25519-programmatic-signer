import {
    argumentValueNode,
    assertIsNode,
    getValidationItemsVisitor,
    instructionRemainingAccountsNode,
    resolverValueNode,
} from 'codama';

const executeRemainingAccounts = instructionRemainingAccountsNode(
    resolverValueNode('resolveMessageAccounts', {
        dependsOn: [argumentValueNode('message')],
        docs: "Preserves each wrapped message account's signer and writable role.",
    }),
    {
        docs: "One account for each key in the wrapped message's static account-key list, in the same order.",
        isOptional: false,
        isSigner: 'either',
    },
);

const submitRemainingAccounts = instructionRemainingAccountsNode(
    resolverValueNode('resolveTransactionAccounts', {
        dependsOn: [argumentValueNode('transaction')],
        docs: "Preserves each wrapped message account's writable role without marking it as an outer Submit signer.",
    }),
    {
        docs: "One account for each key in the wrapped transaction message's static account-key list, in the same order.",
        isOptional: false,
        isSigner: false,
    },
);

const addRemainingAccounts = remainingAccounts => node => {
    assertIsNode(node, 'instructionNode');
    return { ...node, remainingAccounts: [remainingAccounts] };
};

export default {
    idl: 'target/codama-idl/signer-interface.json',
    additionalIdls: [
        'target/codama-idl/executor-interface.json',
        'target/codama-idl/nonce-interface.json',
    ],
    before: [
        {
            from: 'codama#bottomUpTransformerVisitor',
            args: [
                [
                    {
                        select: '[programNode]messageExecutor.[instructionNode]execute',
                        transform: addRemainingAccounts(executeRemainingAccounts),
                    },
                    {
                        select: '[programNode]ed25519Signer.[instructionNode]submit',
                        transform: addRemainingAccounts(submitRemainingAccounts),
                    },
                ],
            ],
        },
        {
            from: 'codama#throwValidatorItemsVisitor',
            args: [getValidationItemsVisitor()],
        },
    ],
    scripts: {
        idl: { from: '@codama/renderers-core#writeIdlVisitor', args: ['idl.json'] },
    },
};
