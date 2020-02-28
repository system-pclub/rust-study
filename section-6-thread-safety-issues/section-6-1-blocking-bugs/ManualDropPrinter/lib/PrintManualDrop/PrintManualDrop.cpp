#include "PrintManualDrop/PrintManualDrop.h"

#include <set>
#include <stack>

#include "llvm/Pass.h"
#include "llvm/Analysis/AliasAnalysis.h"
#include "llvm/IR/DebugInfoMetadata.h"
#include "llvm/IR/DebugLoc.h"
#include "llvm/IR/Instruction.h"
#include "llvm/IR/Instructions.h"
#include "llvm/IR/IntrinsicInst.h"

#include "Common/CallerFunc.h"

#define DEBUG_TYPE "PrintManualDrop"

using namespace llvm;

namespace detector {

    char PrintManualDrop::ID = 0;

    PrintManualDrop::PrintManualDrop() : ModulePass(ID) {}

    void PrintManualDrop::getAnalysisUsage(AnalysisUsage &AU) const {
        AU.setPreservesAll();
    }

    static bool printDebugInfo(Instruction *I) {
        const llvm::DebugLoc &lockInfo = I->getDebugLoc();
//        I->print(errs());
//        errs() << "\n";
        auto di = lockInfo.get();
        if (di) {
            errs() << " " << lockInfo->getDirectory() << ' '
                   << lockInfo->getFilename() << ' '
                   << lockInfo.getLine() << "\n";
            return true;
        } else {
            return false;
        }
    }

    static bool skipInst(Instruction *I) {
        if (!I) {
            return true;
        }
        if (isa<PHINode>(I)) {
            return true;
        }
        if (isa<DbgInfoIntrinsic>(I)) {
            return true;
        }
        return false;
    }

    static bool isLockFunc(Function *F) {
        if (!F) {
            return false;
        }
        StringRef Name = F->getName();
        // Check Mutex
        if (Name.find("mutex") != StringRef::npos || Name.find("Mutex") != StringRef::npos) {
            if (Name.find("raw_mutex") != StringRef::npos|| Name.find("RawMutex") != StringRef::npos) {
                return false;
            } else if (Name.find("GT$4lock") != StringRef::npos) {
                return true;
            }
        } else if (Name.find("rwlock") != StringRef::npos || Name.find("RwLock") != StringRef::npos) {
            if (Name.startswith("HandyRwLock$LT$T$GT$$GT$2rl")
                || Name.startswith("HandyRwLock$LT$T$GT$$GT$2wl")) {
                return true;
            } else if (Name.find("raw_rwlock") != StringRef::npos || Name.find("RawRwLock") != StringRef::npos) {
                return false;
            } else if (Name.find("$GT$4read") != StringRef::npos || Name.find("$GT$5write") != StringRef::npos) {
                return true;
            }
        }
        return false;
    }

    struct stLockInfo {
        Instruction *LockInst;
        Value *ReturnValue;
        Value *LockValue;
    };

    static bool parseLockInst(Instruction *LockInst, stLockInfo &LockInfo) {
        if (!LockInst) {
            return false;
        }
        CallSite CS(LockInst);
        // Mutex
        if (CS.getCalledFunction()->getReturnType()->isVoidTy()) {
            if (CS.getNumArgOperands() > 1) {
                LockInfo.ReturnValue = CS.getArgOperand(0);
                LockInfo.LockValue = CS.getArgOperand(1);
                return true;
            } else {
                errs() << "Void-return Lock\n";
                LockInst->print(errs());
                errs() << "\n";
                return false;
            }
        } else {  // Non-mutex
            LockInfo.ReturnValue = LockInst;
            if (CS.getNumArgOperands() > 0) {
                LockInfo.LockValue = CS.getArgOperand(0);
                return true;
            } else {
                errs() << "Non-parameter Lock\n";
                LockInst->print(errs());
                errs() << "\n";
                return false;
            }
        }
    }

    static bool isDropInst(Instruction *NI) {
        if (isCallOrInvokeInst(NI)) {
            CallSite CS;
            if (Function *F = getCalledFunc(NI, CS)) {
                if (F->getName().startswith("_ZN4core3mem4drop")) {
                    return true;
                }
            }
        }
        return false;
    }

    static bool trackDownToDropInsts(Instruction *RI, std::set<Instruction *> &setDropInst) {
        if (!RI) {
            return false;
        }
        setDropInst.clear();

        std::list<Instruction *> WorkList;
        std::set<Instruction *> Visited;
        WorkList.push_back(RI);
        bool Stop = false;
        while (!WorkList.empty() && !Stop) {
            Instruction *Curr = WorkList.front();
            WorkList.pop_front();
            for (User *U: Curr->users()) {
                if (Instruction *UI = dyn_cast<Instruction>(U)) {
                    if (Visited.find(UI) == Visited.end()) {
                        if (isDropInst(UI)) {
//                            UI->print(errs());
//                            errs() << '\n';
                            setDropInst.insert(UI);
                            Value *V = UI->getOperand(0);
                            assert(V);
                            for (User *UV: V->users()) {
                                if (Instruction *UVI = dyn_cast<Instruction>(UV)) {
                                    if (Visited.find(UVI) == Visited.end()) {
                                        if (isDropInst(UVI)) {
                                            setDropInst.insert(UVI);
                                        }
                                    }
                                }
                            }
                            return true;
                        } else if (StoreInst *SI = dyn_cast<StoreInst>(UI)) {
                            if (Instruction *Dest = dyn_cast<Instruction>(SI->getPointerOperand())) {
                                WorkList.push_back(Dest);
                            } else {
                                errs() << "StoreInst Dest is not a Inst\n";
                                printDebugInfo(Curr);
                            }
                        } else {
                            WorkList.push_back(UI);
                        }
                        Visited.insert(UI);
                    }
                }
            }
        }
        return false;
    }

    static bool parseFunc(Function *F,
                          std::map<Instruction *, Function *> &mapCallInstCallee,
                          std::map<Instruction *, stLockInfo> &mapLockInfo,
                          std::map<Instruction *, std::pair<Function *, std::set<Instruction *>>> &mapLockDropInfo) {
        if (!F || F->isDeclaration()) {
            return false;
        }

        for (BasicBlock &BB : *F) {
            for (Instruction &II : BB) {
                Instruction *I = &II;
                if (!skipInst(I)) {
                    if (isa<CallInst>(I) || isa<InvokeInst>(I)) {
                        CallSite CS(I);
                        Function *Callee = CS.getCalledFunction();
                        if (Callee && !Callee->isDeclaration()) {
                            if (isLockFunc(Callee)) {
                                stLockInfo LockInfo { nullptr, nullptr, nullptr };
                                if (!parseLockInst(I, LockInfo)) {
                                    errs() << "Cannot Parse Lock Inst\n";
                                    printDebugInfo(I);
                                    continue;
                                }
                                Instruction *RI = dyn_cast<Instruction>(LockInfo.ReturnValue);
                                if (!RI) {
                                    errs() << "Return Value is not Inst\n";
                                    LockInfo.ReturnValue->print(errs());
                                    errs() << '\n';
                                    continue;
                                }
                                mapLockInfo[I] = LockInfo;
                                std::set<Instruction *> setDropInst;
                                if (trackDownToDropInsts(RI, setDropInst)) {
                                    mapLockDropInfo[I] = std::make_pair(Callee, setDropInst);
                                    // Debug
                                    errs() << "Manual Drop Info:\n";
//                                    I->print(errs());
                                    printDebugInfo(I);
//                                    errs() << '\n';
                                    for (Instruction *DropInst: setDropInst) {
                                        errs() << '\t';
                                        printDebugInfo(DropInst);
//                                        DropInst->print(errs());
//                                        errs() << '\n';
                                    }
                                } else {
//                                    errs() << "Cannot find Drop for Inst:\n";
//                                    errs() << I->getParent()->getParent()->getName() << '\n';
//                                    I->print(errs());
//                                    printDebugInfo(I);
//                                    errs() << '\n';
                                    mapLockDropInfo[I] = std::make_pair(Callee, setDropInst);
                                }

                            } else {
                                mapCallInstCallee[I] = Callee;
                            }
                        }
                    }
                }
            }
        }

        return true;
    }

    bool PrintManualDrop::runOnModule(Module &M) {
        this->pModule = &M;

        for (Function &F: M) {
            if (F.begin() != F.end()) {
                std::map<Instruction *, Function *> mapCallInstCallee;
                std::map<Instruction *, stLockInfo> mapLockInfo;
                std::map<Instruction *, std::pair<Function *, std::set<Instruction *>>> mapLockDropInfo;
                parseFunc(&F, mapCallInstCallee, mapLockInfo, mapLockDropInfo);
            }
        }
        return false;
    }
}

static RegisterPass<detector::PrintManualDrop> X(
        "print",
        "Print related ManualDrop funcs",
        false,
        true);