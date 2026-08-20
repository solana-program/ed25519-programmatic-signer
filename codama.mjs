import { writeIdlVisitor } from '@codama/renderers-core';
import {
    argumentValueNode,
    getValidationItemsVisitor,
    instructionRemainingAccountsNode,
    resolverValueNode,
    rootNodeVisitor,
    throwValidatorItemsVisitor,
    visit,
} from 'codama';

const NONCE_PROGRAM = [
    'Noncediea1fH12usShuQAz28UhgAeuE5Maf32LsMUQB',
    {
        output: 'nonce/interface/idl.json',
    },
];

const EXECUTOR_PROGRAM = [
    'ExecxgyHYsAXB4c5dZodV1zJZ9hqfsDCYkRDRATrpkFR',
    {
        output: 'executor/interface/idl.json',
        remainingAccounts: {
            argument: 'message',
            docs: "One account for each key in the wrapped message's static account-key list, in the same order.",
            instruction: 'execute',
            isSigner: 'either',
            resolver: 'resolveMessageAccounts',
            resolverDocs:
                "Preserves each wrapped message account's signer and writable role.",
        },
    },
];

const SIGNER_PROGRAM = [
    'EdSigVfK1DkeMrjFNDMjwfQaJPhPTtX7jW8uPv3oKEgN',
    {
        output: 'signer/interface/idl.json',
        remainingAccounts: {
            argument: 'transaction',
            docs: "One account for each key in the wrapped transaction message's static account-key list, in the same order.",
            instruction: 'submit',
            isSigner: false,
            resolver: 'resolveTransactionAccounts',
            resolverDocs:
                "Preserves each wrapped message account's writable role without marking it as an outer Submit signer.",
        },
    },
];

const PROGRAMS = new Map([NONCE_PROGRAM, EXECUTOR_PROGRAM, SIGNER_PROGRAM]);

const getProgramConfig = root => PROGRAMS.get(root.program.publicKey);

export const enrichIdlVisitor = () =>
    rootNodeVisitor(root => {
        const config = getProgramConfig(root).remainingAccounts;
        if (!config) return root;

        const remainingAccounts = instructionRemainingAccountsNode(
            resolverValueNode(config.resolver, {
                dependsOn: [argumentValueNode(config.argument)],
                docs: config.resolverDocs,
            }),
            {
                docs: config.docs,
                isOptional: false,
                isSigner: config.isSigner,
            },
        );

        return {
            ...root,
            program: {
                ...root.program,
                instructions: root.program.instructions.map(candidate =>
                    candidate.name === config.instruction
                        ? { ...candidate, remainingAccounts: [remainingAccounts] }
                        : candidate,
                ),
            },
        };
    });

export const validateIdlVisitor = () =>
    throwValidatorItemsVisitor(getValidationItemsVisitor());

export const writeProgramIdlVisitor = () =>
    rootNodeVisitor(root => visit(root, writeIdlVisitor(getProgramConfig(root).output)));

export default {
    before: ['./codama.mjs#enrichIdlVisitor', './codama.mjs#validateIdlVisitor'],
    scripts: {
        idl: './codama.mjs#writeProgramIdlVisitor',
    },
};
