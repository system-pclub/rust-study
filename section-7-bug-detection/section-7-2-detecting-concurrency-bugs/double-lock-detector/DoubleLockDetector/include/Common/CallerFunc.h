#ifndef PRINTPASS_GETCALLERFUNC_H
#define PRINTPASS_GETCALLERFUNC_H

#include "llvm/IR/Instruction.h"
#include "llvm/IR/Function.h"
#include "llvm/IR/CallSite.h"

bool isCallOrInvokeInst(llvm::Instruction *I);

llvm::Function *getCalledFunc(llvm::Instruction *pInst, llvm::CallSite &CS);

#endif //PRINTPASS_GETCALLERFUNC_H
