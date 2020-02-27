#include "RustDoubleLockDetector/RustDoubleLockDetector.h"

#include "llvm/Pass.h"
#include "llvm/Analysis/AliasAnalysis.h"
#include "llvm/Analysis/ValueTracking.h"
#include "llvm/IR/DebugInfoMetadata.h"
#include "llvm/IR/DebugLoc.h"
#include "llvm/IR/Instruction.h"
#include "llvm/IR/Instructions.h"
#include "llvm/IR/IntrinsicInst.h"
#include "llvm/IR/Operator.h"

#include <set>
#include <stack>
#include <unordered_map>

#include "Common/CallerFunc.h"

#define DEBUG_TYPE "RustDoubleLockDetector"
#define STDRWLOCK 1
#define LOCKAPI 1
#define STDMUTEX 1
using namespace llvm;

namespace detector {

    char RustDoubleLockDetector::ID = 0;

    RustDoubleLockDetector::RustDoubleLockDetector() : ModulePass(ID) {
        PassRegistry &Registry = *PassRegistry::getPassRegistry();
        initializeAAResultsWrapperPassPass(Registry);
    }

    void RustDoubleLockDetector::getAnalysisUsage(AnalysisUsage &AU) const {
        AU.setPreservesAll();
        AU.addRequired<AAResultsWrapperPass>();
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

    static bool printDebugInfo(Instruction *I) {
        const llvm::DebugLoc &lockInfo = I->getDebugLoc();
        // errs() << I->getParent()->getName() << ":";
        // I->print(errs());
        // errs() << "\n";
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

    static bool collectGlobalCallSite(
            Function *F,  // Input
            std::map<Instruction *, Function *> &mapCallSite  // Output
    ) {
        if (!F || F->isDeclaration()) {
            return false;
        }
        for (BasicBlock &B : *F) {
            for (Instruction &II : B) {
                Instruction *I = &II;
                if (skipInst(I)) {
                    continue;
                }
                if (isCallOrInvokeInst(I)) {
                    CallSite CS;
                    if (Function *Callee = getCalledFunc(I, CS)) {
                        mapCallSite[I] = Callee;
                    }
                }
            }
        }
        return true;
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

    static bool isLockAPIRwLockRead(StringRef FuncName) {
        return FuncName.startswith("_ZN8lock_api6rwlock19RwLock$LT$R$C$T$GT$4read17h") ||
        FuncName.startswith("_ZN8lock_api6rwlock19RwLock$LT$R$C$T$GT$5write17h") ||
        FuncName.startswith("_ZN8lock_api5mutex18Mutex$LT$R$C$T$GT$4lock17h");
    }

    static bool isStdLock(StringRef FuncName) {
        return FuncName.startswith("_ZN3std4sync5mutex14Mutex$LT$T$GT$4lock17h");
    }

    static bool isStdRead(StringRef FuncName) {
        return FuncName.startswith("_ZN3std4sync6rwlock15RwLock$LT$T$GT$4read17h");
    }

    static bool isStdWrite(StringRef FuncName) {
        return FuncName.startswith("_ZN3std4sync6rwlock15RwLock$LT$T$GT$5write17h");
    }

    static bool isAutoDropAPI(StringRef FuncName) {
        return FuncName.startswith("_ZN4core3ptr18real_drop_in_place17h");
    }

    static bool isManualDropAPI(StringRef FuncName) {
        return FuncName.startswith("_ZN4core3mem4drop17h");
    }

    static bool isResultToInnerAPI(StringRef FuncName) {
        return FuncName.startswith("_ZN4core6result19Result$LT$T$C$E$GT$6unwrap17h")
               || FuncName.startswith("_ZN4core6result19Result$LT$T$C$E$GT$9unwrap_or17h")
               || FuncName.startswith("_ZN4core6result19Result$LT$T$C$E$GT$14unwrap_or_else17h")
               || FuncName.startswith("_ZN4core6result19Result$LT$T$C$E$GT$17unwrap_or_default17h")
               || FuncName.startswith("_ZN4core6result19Result$LT$T$C$E$GT$6expect17h");
    }

    struct LockInfo {
        Instruction *LockInst;
        Value *LockValue;
        Value *ResultValue;

        LockInfo() :
            LockInst(nullptr),
            LockValue(nullptr),
            ResultValue(nullptr) {
        }
    };

    struct MutexSource {
        Value *direct;
        Type *structTy;
        std::vector<APInt> index;

        bool operator==(const MutexSource& rhs) const {
            if (this->structTy != rhs.structTy) {
                return false;
            }
            if (this->index.size() != rhs.index.size()) {
                return false;
            }
            for (std::size_t i = 0; i < this->index.size(); ++i) {
                if (this->index[i] != rhs.index[i]) {
                    return false;
                }
            }
            return true;
        }

        void print(llvm::raw_ostream &os) const {
            this->structTy->print(os);
            os << "\n";
            for (std::size_t i = 0; i < this->index.size(); ++i) {
                os << i << ","; 
            }
            os << "\n";
        }
    };

    struct MutexSourceHasher {
        std::size_t operator()(const MutexSource& k) const {
            std::size_t hash = llvm::hash_value(k.structTy);
            for (APInt i : k.index) {
                hash ^= llvm::hash_value(i);
            }
            return hash;
        }
    };

    static bool traceMutexSource(Value *mutex, MutexSource &MS) {
        assert(mutex);

        MS.direct = mutex;
        for (auto it = mutex->use_begin(); it != mutex->use_end(); ++it) {
            if (GetElementPtrInst *GEP = dyn_cast<GetElementPtrInst>(it->get())) {
                Value *Self = GEP->getOperand(0);
                Type *structTy = Self->stripPointerCasts()->getType()->getContainedType(0);
                // structTy->print(errs());
                // errs() << "\n";
                if (!isa<StructType>(structTy)) {
                    errs() << "Self not Struct" << "\n";
                    continue;
                }
                MS.structTy = structTy;
                for (unsigned i = 1; i < GEP->getNumOperands(); ++i) {
                    // errs() << "index: ";
                    APInt idx = dyn_cast<ConstantInt>(GEP->getOperand(i))->getValue();
                    MS.index.push_back(idx);
                    // GEP->getOperand(i)->getType()->print(errs());
                    // errs() << "\n";
                    // GEP->getOperand(i)->print(errs());
                    // errs() << "\n";
                }
                return true;
            } else if (BitCastOperator *BCO = dyn_cast<BitCastOperator>(it->get())) {
                // TODO;
                Value *Self = BCO->getOperand(0);
                Type *structTy = Self->stripPointerCasts()->getType()->getContainedType(0);
                // structTy->print(errs());
                // errs() << "\n";
                if (!isa<StructType>(structTy)) {
                    errs() << "Self not Struct" << "\n";
                    continue;
                }
                MS.structTy = structTy;
                return true;
            }
        }
        return false;
    }

    static void traceDropInstForInstruction(Instruction *Inst, std::set<Instruction *> &setDropInst) {
        for (User *UL : Inst->users()) {
            Instruction *I = dyn_cast<Instruction>(UL);
            if (!I) {
                continue;
            }
            if (skipInst(I)) {
                continue;
            }
            if (isCallOrInvokeInst(I)) {
                CallSite CS(I);
                Function *F = CS.getCalledFunction();
                if (!F) {
                    continue;
                }
                if (isAutoDropAPI(F->getName())
                    || isManualDropAPI(F->getName())) {
                    setDropInst.insert(I);
                }
            }
        }
    }

    static void traceDropInst(LockInfo &MLI, std::set<Instruction *> &setDropInst) {
        Value *LockGuardValue = MLI.ResultValue;
        for (User *U : LockGuardValue->users()) {
            Instruction *I = dyn_cast<Instruction>(U);
            if (I == MLI.LockInst) {
                continue;
            }
            if (!I) {
                continue;
            }
            if (skipInst(I)) {
                continue;
            }
            
            if (isCallOrInvokeInst(I)) {
                CallSite CS(I);
                Function *F = CS.getCalledFunction();
                if (!F) {
                    continue;
                }
                if (isAutoDropAPI(F->getName())
                    || isManualDropAPI(F->getName())) {
                    setDropInst.insert(I);
                }
            } else if (LoadInst *LI = dyn_cast<LoadInst>(I)) {
                for (User *UL : LI->users()) {
                    Instruction *I = dyn_cast<Instruction>(UL);
                    if (!I) {
                        continue;
                    }
                    if (skipInst(I)) {
                        continue;
                    }
                    if (isCallOrInvokeInst(I)) {
                        CallSite CS(I);
                        Function *F = CS.getCalledFunction();
                        if (!F) {
                            continue;
                        }
                        if (isAutoDropAPI(F->getName())
                        || isManualDropAPI(F->getName())) {
                            setDropInst.insert(I);
                        }
                    }
                }
            } else if (StoreInst *SI = dyn_cast<StoreInst>(I)) {
                Value *Target = SI->getPointerOperand();
                for (User *UL : Target->users()) {
                    Instruction *I = dyn_cast<Instruction>(UL);
                    if (!I) {
                        continue;
                    }
                    if (skipInst(I)) {
                        continue;
                    }
                    if (isCallOrInvokeInst(I)) {
                        CallSite CS(I);
                        Function *F = CS.getCalledFunction();
                        if (!F) {
                            continue;
                        }
                        if (isAutoDropAPI(F->getName())
                        || isManualDropAPI(F->getName())) {
                            setDropInst.insert(I);
                        }
                    } else if (LoadInst *LI = dyn_cast<LoadInst>(UL)) {
                        traceDropInstForInstruction(LI, setDropInst);
                    }
                }
            }
        }
    }

    static bool isGEP01(Instruction *I) {
        GetElementPtrInst *GEP = dyn_cast<GetElementPtrInst>(I);
        if (!GEP) {
            return false;
        }
        if (GEP->getNumOperands() < 3) {
            return false;
        }
        APInt idx0 = dyn_cast<ConstantInt>(GEP->getOperand(1))->getValue();
        if (idx0 != 0) {
            return false;
        }
        APInt idx1 = dyn_cast<ConstantInt>(GEP->getOperand(2))->getValue();
        if (idx1 != 1) {
            return false; 
        }
        return true; 
    }

    static bool isGEP00(Instruction *I) {
        GetElementPtrInst *GEP = dyn_cast<GetElementPtrInst>(I);
        if (!GEP) {
            return false;
        }
        if (GEP->getNumOperands() < 3) {
            return false;
        }
        APInt idx0 = dyn_cast<ConstantInt>(GEP->getOperand(1))->getValue();
        if (idx0 != 0) {
            return false;
        }
        APInt idx1 = dyn_cast<ConstantInt>(GEP->getOperand(2))->getValue();
        if (idx1 != 0) {
            return false; 
        }
        return true; 
    }

    static bool isDropInst(Instruction *I) {
        if (!isCallOrInvokeInst(I)) {
            return false;
        }
        CallSite CS(I);
        Function *F = CS.getCalledFunction();
        if (!F) {
            return false; 
        }
        if (isAutoDropAPI(F->getName())
            || isManualDropAPI(F->getName())) {
            return true; 
        }
        return false; 
    }

    static bool isICmpInst0(Instruction *I) {
        ICmpInst *ICmp = dyn_cast<ICmpInst>(I);
        if (!ICmp) {
            return false;
        }
        ConstantInt *CI = dyn_cast<ConstantInt>(ICmp->getOperand(1));
        if (!CI) {
            return false;
        }
        APInt num = CI->getValue();
        if (num == 0) {
            return true;
        }
        return false; 
    }

    static bool getICmp0Br0First(Instruction *ICmp, std::set<Instruction *> &setFirst) {
        for (User *U : ICmp->users()) {
            if (BranchInst *BI = dyn_cast<BranchInst>(U)) {
                Value *V = BI->getOperand(1);
                if (BasicBlock *B = dyn_cast<BasicBlock>(V)) {
                    setFirst.insert(B->getFirstNonPHIOrDbgOrLifetime());
                }
            }    
        }
        return !setFirst.empty(); 
    }

    static void visitUsersOfValue(Value *V, bool (*F)(Instruction *), std::set<Instruction *>& setOut) {
        for (User *U : V->users()) {
            if (Instruction *I = dyn_cast<Instruction>(U)) {
                if (F(I)) {
                    setOut.insert(I);
                    //if (F == &isDropInst) {
                    //    errs() << "DropInst:\n"; I->print(errs()); errs() << "\n"; 
                    //}
                }
            }
        }
   }

    static void traceResult(LockInfo &MLI, std::set<Instruction *> &setDropInst, const DataLayout &DL) {
        Value *ResultValue = MLI.ResultValue;
        for (User *U : ResultValue->users()) {
            Instruction *I = dyn_cast<Instruction>(U);
            if (I == MLI.LockInst) {
                continue;
            }
            if (!I) {
                continue;
            }
            if (skipInst(I)) {
                continue;
            }
            
            if (isCallOrInvokeInst(I)) {
                CallSite CS(I);
                Function *F = CS.getCalledFunction();
                if (!F) {
                    continue;
                }
                if (isAutoDropAPI(F->getName())
                    || isManualDropAPI(F->getName())) {
                    setDropInst.insert(I);
                } else if (isResultToInnerAPI(F->getName())) {
                    Value *LockGuardValue;
                    if (F->getReturnType()->isVoidTy()) {
                        LockGuardValue = GetUnderlyingObject(I->getOperand(0), DL);
                    } else {
                        LockGuardValue = I;
                    }
                    MLI.ResultValue = LockGuardValue;
                    traceDropInst(MLI, setDropInst);
                }
            } else if (LoadInst *LI = dyn_cast<LoadInst>(I)) {
                for (User *UL : LI->users()) {
                    Instruction *I = dyn_cast<Instruction>(UL);
                    if (!I) {
                        continue;
                    }
                    if (skipInst(I)) {
                        continue;
                    }
                    if (isCallOrInvokeInst(I)) {
                        CallSite CS(I);
                        Function *F = CS.getCalledFunction();
                        if (!F) {
                            continue;
                        }
                        if (isAutoDropAPI(F->getName())
                        || isManualDropAPI(F->getName())) {
                            setDropInst.insert(I);
                        }
                    }
                }
            } else if (StoreInst *SI = dyn_cast<StoreInst>(I)) {
                Value *Target = SI->getPointerOperand();
                for (User *UL : Target->users()) {
                    Instruction *I = dyn_cast<Instruction>(UL);
                    if (!I) {
                        continue;
                    }
                    if (skipInst(I)) {
                        continue;
                    }
                    if (isCallOrInvokeInst(I)) {
                        CallSite CS(I);
                        Function *F = CS.getCalledFunction();
                        if (!F) {
                            continue;
                        }
                        if (isAutoDropAPI(F->getName())
                        || isManualDropAPI(F->getName())) {
                            setDropInst.insert(I);
                        }
                    } else if (LoadInst *LI = dyn_cast<LoadInst>(UL)) {
                        traceDropInstForInstruction(LI, setDropInst);
                    }
                }
            } else if (BitCastInst *BCI = dyn_cast<BitCastInst>(I)) {
                //errs() << "BitCastInst" << "\n";
                //BCI->print(errs());
                //errs() << "\n";
                std::set<Instruction *> setCastLoad;
                visitUsersOfValue(BCI, [](Instruction *I) { return isa<LoadInst>(I); }, setCastLoad);
                std::set<Instruction *> setICmp0;
                for (Instruction *CastLoad : setCastLoad) {
                    visitUsersOfValue(CastLoad, [](Instruction *I) { return isa<ICmpInst>(I); }, setICmp0);
                }
                for (Instruction *ICmp0 : setICmp0) {
                    //errs() << "ICmp0:\n";
                    //ICmp0->print(errs());
                    //errs() << "\n";
                    getICmp0Br0First(ICmp0, setDropInst);
                }
                std::set<Instruction *> setGEP01;
                visitUsersOfValue(BCI, isGEP01, setGEP01);
                //for (Instruction *GEP01 : setGEP01) {
                //    errs() << "GEP01\n";
                //    GEP01->print(errs());
                //    errs() << "\n";
                //}
                for (Instruction *LockGuard: setGEP01) {
                    visitUsersOfValue(LockGuard, isDropInst, setDropInst);     
                }
                std::set<Instruction *> setGEP00;
                for (Instruction *GEP01 : setGEP01) {
                    visitUsersOfValue(GEP01, isGEP00, setGEP00);
                }
                //for (Instruction *GEP00 : setGEP00) {
                //    errs() << "GEP00\n";
                //    GEP00->print(errs());
                //    errs() << "\n";
                //}
                std::set<Instruction *> setLoad;
                for (Instruction *GEP00: setGEP00) {
                    visitUsersOfValue(GEP00, [](Instruction *I) { return isa<LoadInst>(I); }, setLoad);
                }
                //for (Instruction *Load : setLoad) {
                //    errs() << "Load\n";
                //    Load->print(errs());
                //    errs() << "\n";
                //}
                std::set<Instruction *> setStore;
                for (Instruction *Load: setLoad) {
                    visitUsersOfValue(Load, [](Instruction *I) { return isa<StoreInst>(I); }, setStore);
                }
                //for (Instruction *Store : setStore) {
                //    errs() << "Store\n";
                //    Store->print(errs());
                //    errs() << "\n";
                //}
                //errs() << "Store Target\n";
                std::set<Instruction *> setGEPGuard;
                for (Instruction *Store: setStore) {
                    Value *TargetAddr = Store->getOperand(1);
                    Value *Target = GetUnderlyingObject(TargetAddr, DL);
                    //Target->print(errs());
                    //errs() << "\n";
                    visitUsersOfValue(Target, [](Instruction *I) { return isa<GetElementPtrInst>(I); }, setGEPGuard);
                }
                //for (Instruction *GEPGuard : setGEPGuard) {
                //    errs() << "GEPGuard\n";
                //    GEPGuard->print(errs());
                //    errs() << "\n";
                //}
                std::set<Instruction *> setLockGuard;
                for (Instruction *GEPGuard: setGEPGuard) {
                    if (Instruction *LockGuard = dyn_cast<Instruction>(GEPGuard->getOperand(0))) {
                        setLockGuard.insert(LockGuard);
                    }
                }
                for (Instruction *LockGuard: setLockGuard) {
                    //errs() << "LockGuard" << "\n";
                    //errs() << LockGuard->getParent()->getName() << "\n";
                    //LockGuard->print(errs());
                    //errs() << "\n";
                    visitUsersOfValue(LockGuard, isDropInst, setDropInst);     
                }
                std::set<Instruction *> setLoadLockGuard;
                for (Instruction *LockGuard: setLockGuard) {
                    visitUsersOfValue(LockGuard, [](Instruction *I) { return isa<LoadInst>(I); }, setLoadLockGuard);
                }
                for (Instruction *LoadLockGuard: setLoadLockGuard) {
                    visitUsersOfValue(LoadLockGuard, isDropInst, setDropInst);     
                }
            }
        }
    }

        static bool trackCallee(Instruction *LockInst,
                            std::pair<Instruction *, Function *> &DirectCalleeSite,
                            std::map<Function *, std::map<Instruction *, Function *>> &mapCallerCallees,
                            std::map<Function *, std::set<Instruction *>> &mapAliasFuncLock) {

        bool HasDoubleLock = false;

        Function *DirectCallee = DirectCalleeSite.second;

        if (mapAliasFuncLock.find(DirectCallee) != mapAliasFuncLock.end()) {
            // Restore
           HasDoubleLock = true;
           errs() << "Double Lock Happens! First Lock:\n";
           printDebugInfo(LockInst);
        //    errs() << DirectCallee->getName() << '\n';
           // Debug Require
        //    LockInst->print(errs());
        //    errs() << '\n';
           //printDebugInfo(DirectCalleeSite.first);
           errs() << "Second Lock(s):\n";
           for (Instruction *AliasLock : mapAliasFuncLock[DirectCallee]) {
               printDebugInfo(AliasLock);
            //    AliasLock->print(errs());
            //    errs() << '\n';
           }
           errs() << '\n';
        }

        std::stack<Function *> WorkList;
        std::set<Function *> Visited;

        std::map<Function *, Instruction *> mapParentInst;

        WorkList.push(DirectCallee);
        Visited.insert(DirectCallee);

        mapParentInst[DirectCallee] = DirectCalleeSite.first;

        while (!WorkList.empty()) {
            Function *Curr = WorkList.top();
            WorkList.pop();
//            errs() << Curr->getName() << '\n';
            auto itCallerCallInst = mapCallerCallees.find(Curr);
            if (itCallerCallInst != mapCallerCallees.end()) {
//                errs() << "Caller Found\n";
                std::map<Instruction *, Function *> &mapCallInstCallee = itCallerCallInst->second;
                for (auto &CallInstCallee : mapCallInstCallee) {
                    Instruction *CallInst = CallInstCallee.first;
                    Function *Callee = CallInstCallee.second;
//                    errs() << "Callee Found " << Callee->getName() << '\n';
                    if (Visited.find(Callee) == Visited.end()) {
//                        errs() << "Not Visited\n";
                        if (mapAliasFuncLock.find(Callee) != mapAliasFuncLock.end()) {
                            // Restore
                           errs() << "Double Lock Happens! First Lock:\n";
                        //    errs() << LockInst->getParent()->getParent()->getName() << '\n';
                           printDebugInfo(LockInst);
                        //    LockInst->print(errs());
                        //    errs() << '\n';
                        //    errs() << Callee->getName() << '\n';
                           errs() << "Second Lock(s):\n";
                           for (Instruction *AliasLock : mapAliasFuncLock[Callee]) {
                               printDebugInfo(AliasLock);
                            //    LockInst->print(errs());
                            //    errs() << '\n';
                           }
                           errs() << '\n';
                            // backtrace print
                            mapParentInst[Callee] = CallInst;
                            std::set<Function *> TraceVisited;
                            auto it = mapParentInst.find(Callee);
                            while (it != mapParentInst.end()) {
                                Instruction *ParentInst = it->second;
                                printDebugInfo(ParentInst);
                                // errs() << ParentInst->getParent()->getName() << ": ";
                                // ParentInst->print(errs());
                                // errs() << '\n';
                                Function *ParentFunc = ParentInst->getParent()->getParent();
                                it = mapParentInst.find(ParentFunc);
                                if (it != mapParentInst.end()) {
                                    if (TraceVisited.find(it->first) != TraceVisited.end()) {
                                        break;
                                    } else {
                                        TraceVisited.insert(it->first);
                                    }
                                }
                            }
                            // end of backtrack
                            HasDoubleLock = true;
                        }
                        WorkList.push(Callee);
                        mapParentInst[Callee] = CallInst;
                        Visited.insert(Callee);
                    }
                }
            }
        }

        return HasDoubleLock;
    }

    static bool trackLockInst(Instruction *LockInst,
                              std::set<Instruction *> setMayAliasLock,
                              std::set<Instruction *> setDrop,
                              std::map<Function *, std::map<Instruction *, Function *>> &mapCallerCallees) {

//        std::set<Function *> setMayAliasFunc;
//        for (Instruction *I : setMayAliasLock) {
//            if (I != LockInst) {
//                setMayAliasFunc.insert(I->getParent()->getParent());
//            }
//        }
        std::map<Function *, std::set<Instruction *>> mapMayAliasFuncLock;
        for (Instruction *I : setMayAliasLock) {
            if (I != LockInst) {
                Function *AliasFunc = I->getParent()->getParent();
                mapMayAliasFuncLock[AliasFunc].insert(I);
            }
        }
//        // Debug
//        printDebugInfo(LockInst);
//        for (Function *F : setMayAliasFunc) {
//            errs() << F->getName() << '\n';
//        }
        std::stack<BasicBlock *> WorkList;
        std::set<BasicBlock *> Visited;

        Function *Caller = LockInst->getParent()->getParent();
        auto &mapCallInstCallee = mapCallerCallees[Caller];

//        // Debug
//        for (auto &kv : mapCallInstCallee) {
//            kv.first->print(errs());
//            errs() << '\n';
//        }

        if (Caller->getName().startswith("_ZN12ethcore_sync10light_sync18LightSync$LT$L$GT$13maintain_sync17h")) {
            return false;
        }
        //errs() << "Begin:\n";
        //errs() << LockInst->getFunction()->getName() << "\n";
        BasicBlock *LockInstBB = LockInst->getParent();
        Visited.insert(LockInstBB);
        Instruction *pTerm = LockInstBB->getTerminator();
        if (pTerm->getNumSuccessors() >= 1) {
            BasicBlock *NextBB = pTerm->getSuccessor(0);
            WorkList.push(NextBB);  // no unwind
            Visited.insert(NextBB);
        }
        while (!WorkList.empty()) {
            BasicBlock *Curr = WorkList.top();
            //errs() << Curr->getName() << "\n";
            WorkList.pop();
            bool StopPropagation = false;
            for (Instruction &II: *Curr) {
                Instruction *I = &II;
                if (I == LockInst) {
                    continue;
                }
                // contains same Lock
                if (setMayAliasLock.find(I) != setMayAliasLock.end()) {
                    // Restore
                   errs() << "Double Lock Happens! First Lock:\n";
                   printDebugInfo(LockInst);
                   errs() << "Second Lock(s):\n";
                   printDebugInfo(I);
                   // Debug Require
                   // LockInst->print(errs());
                   errs() << '\n';
                    StopPropagation = true;
                    // break;
                } else if (setDrop.find(I) != setDrop.end()) {
                    // contains same Drop
                    //errs() << "dropped\n";
                    StopPropagation = true;
                    break;
                } else {
                    // is a CallInst
                    auto it = mapCallInstCallee.find(I);
                    if (it == mapCallInstCallee.end()) {
                        continue;
                    } else {
                        Instruction *CI = it->first;
                        Function *Callee = it->second;
//                        errs() << Callee->getName() << "\n";
                        auto CalleeSite = std::make_pair(CI, Callee);
//                        if (trackCallee(LockInst, CalleeSite, mapCallerCallees, setMayAliasFunc)) {
//                            StopPropagation = true;
//                            break;
//                        }
                        if (trackCallee(LockInst, CalleeSite, mapCallerCallees, mapMayAliasFuncLock)) {
                            StopPropagation = true;
                            break;
                        }
                    }
                }
            }

            if (!StopPropagation) {
                Instruction *pTerm = Curr->getTerminator();
                for (unsigned i = 0; i < pTerm->getNumSuccessors(); ++i) {
                    BasicBlock *Succ = pTerm->getSuccessor(i);
                    // if (Succ->getName() == "_ZN3log9max_level17h461ecf87d921ec18E.exit92") {
                    //     continue;
                    // }
                    if (isa<LandingPadInst>(Succ->getFirstNonPHIOrDbgOrLifetime())) {
                         continue;
                    }
                    if (Visited.find(Succ) == Visited.end()) {
                        WorkList.push(Succ);
                        Visited.insert(Succ);
                    }
                }
            }
        }

        return true;
    }

    static bool trackLockInstLocal(Instruction *LockInst,
                              std::set<Instruction *> setMayAliasLock,
                              std::set<Instruction *> setDrop) {

//        std::set<Function *> setMayAliasFunc;
//        for (Instruction *I : setMayAliasLock) {
//            if (I != LockInst) {
//                setMayAliasFunc.insert(I->getParent()->getParent());
//            }
//        }
        std::map<Function *, std::set<Instruction *>> mapMayAliasFuncLock;
        for (Instruction *I : setMayAliasLock) {
            if (I != LockInst) {
                Function *AliasFunc = I->getParent()->getParent();
                mapMayAliasFuncLock[AliasFunc].insert(I);
            }
        }
//        // Debug
//        printDebugInfo(LockInst);
//        for (Function *F : setMayAliasFunc) {
//            errs() << F->getName() << '\n';
//        }
        std::stack<BasicBlock *> WorkList;
        std::set<BasicBlock *> Visited;

        Function *Caller = LockInst->getParent()->getParent();

//        // Debug
//        for (auto &kv : mapCallInstCallee) {
//            kv.first->print(errs());
//            errs() << '\n';
//        }

        if (Caller->getName().startswith("_ZN12ethcore_sync10light_sync18LightSync$LT$L$GT$13maintain_sync17h")) {
            return false;
        }
        // errs() << "Begin:\n";
        // errs() << LockInst->getFunction()->getName() << "\n";
        BasicBlock *LockInstBB = LockInst->getParent();
        Visited.insert(LockInstBB);
        Instruction *pTerm = LockInstBB->getTerminator();
        if (pTerm->getNumSuccessors() >= 1) {
            BasicBlock *NextBB = pTerm->getSuccessor(0);
            WorkList.push(NextBB);  // no unwind
            Visited.insert(NextBB);
        }
        CallSite CS(LockInst);
        Function *LockFunc = CS.getCalledFunction();
        if (!LockFunc) {
            return false;
        }
        bool FirstRead = false;
        if (isStdRead(LockFunc->getName())) {
            FirstRead = true;
        }
        while (!WorkList.empty()) {
            BasicBlock *Curr = WorkList.top();
            // errs() << Curr->getName() << "\n";
            WorkList.pop();
            bool StopPropagation = false;
            for (Instruction &II: *Curr) {
                Instruction *I = &II;
                if (I == LockInst) {
                    continue;
                }
                // contains same Lock
                if (setMayAliasLock.find(I) != setMayAliasLock.end()) {
                    if (FirstRead) {
                        CallSite CS(I);
                        Function *SecondLockFunc = CS.getCalledFunction();
                        if (!SecondLockFunc) {
                            continue;
                        }
                        if (isStdRead(SecondLockFunc->getName())) {
                            continue;
                        }
                    }
                    // Restore
                   errs() << "Double Lock Happens! First Lock:\n";
                   printDebugInfo(LockInst);
                   errs() << "Second Lock(s):\n";
                   printDebugInfo(I);
                   // Debug Require
                   // LockInst->print(errs());
                   errs() << '\n';
                    StopPropagation = true;
                    // break;
                } else if (setDrop.find(I) != setDrop.end()) {
                    // contains same Drop
                    StopPropagation = true;
                    break;
                }
            }

            if (!StopPropagation) {
                Instruction *pTerm = Curr->getTerminator();
                for (unsigned i = 0; i < pTerm->getNumSuccessors(); ++i) {
                    BasicBlock *Succ = pTerm->getSuccessor(i);
                    // if (Succ->getName() == "_ZN3log9max_level17h461ecf87d921ec18E.exit92") {
                    //     continue;
                    // }
                    if (isa<LandingPadInst>(Succ->getFirstNonPHIOrDbgOrLifetime())) {
                         continue;
                    }
                    if (Visited.find(Succ) == Visited.end()) {
                        WorkList.push(Succ);
                        Visited.insert(Succ);
                    }
                }
            }
        }

        return true;
    }


    static void parseLockAPIRwLockRead(Instruction *LockInst, LockInfo &LI) {
        assert(LockInst);
        CallSite CS(LockInst);
        assert(CS.getNumArgOperands() >= 1);
        LI.LockValue = CS.getArgOperand(0);
        LI.LockInst = LockInst;
        LI.ResultValue = LockInst;
    }

    static void parseStdRead(Instruction *LockInst, LockInfo &LI) {
        assert(LockInst);
        CallSite CS(LockInst);
        assert(CS.getNumArgOperands() >= 1);
        for (auto it = LockInst->user_begin(); it != LockInst->user_end(); ++it) {
            if (isa<ExtractValueInst>(*it)) {
                LI.ResultValue = *it;
                break;
            }
        }
        if (!LI.ResultValue) {
            LI.ResultValue = LockInst;
        }
        LI.LockValue = CS.getArgOperand(0);
        LI.LockInst = LockInst;
        LI.ResultValue = LockInst;
    }

    static void parseStdLockWrite(Instruction *LockInst, LockInfo &LI) {
        assert(LockInst);
        CallSite CS(LockInst);
        assert(CS.getNumArgOperands() >= 2);
        LI.LockValue = CS.getArgOperand(1);
        LI.LockInst = LockInst;
        LI.ResultValue = CS.getArgOperand(0);
    }

    bool RustDoubleLockDetector::runOnModule(Module &M) {
        this->pModule = &M;

        std::map<Function *, std::map<Instruction *, Function *>> mapGlobalCallSite;
        for (Function &F : M) {
            collectGlobalCallSite(&F, mapGlobalCallSite[&F]);
        }

        std::map<Function *, std::set<Instruction *>> mapCalleeToCallSites;
        for (auto &CallerCallSites : mapGlobalCallSite) {
            for (auto &CallInstCallee : CallerCallSites.second) {
                mapCalleeToCallSites[CallInstCallee.second].insert(CallInstCallee.first);
            }
        }

        std::map<Function *, std::map<Instruction *, Function *>> mapLockAPIRwLockRead;
        std::map<Function *, std::map<Instruction *, Function *>> mapStdRead;
        std::map<Function *, std::map<Instruction *, Function *>> mapStdWrite;
        std::map<Function *, std::map<Instruction *, Function *>> mapStdLock;
        for (auto &CallerCallSites : mapGlobalCallSite) {
            for (auto &CallInstCallee : CallerCallSites.second) {
                auto FuncName = CallInstCallee.second->getName();
                if (isLockAPIRwLockRead(FuncName)) {
                    if (mapLockAPIRwLockRead.find(CallerCallSites.first) == mapLockAPIRwLockRead.end()) {
                        mapLockAPIRwLockRead[CallerCallSites.first] = std::map<Instruction *, Function *>();
                    }
                    mapLockAPIRwLockRead[CallerCallSites.first][CallInstCallee.first] = CallInstCallee.second;
                } else if (isStdLock(FuncName)) {
                    if (mapStdLock.find(CallerCallSites.first) == mapStdLock.end()) {
                        mapStdLock[CallerCallSites.first] = std::map<Instruction *, Function *>();
                    }
                    mapStdLock[CallerCallSites.first][CallInstCallee.first] = CallInstCallee.second;
                } else if (isStdRead(FuncName)) {
                    if (mapStdRead.find(CallerCallSites.first) == mapStdRead.end()) {
                        mapStdRead[CallerCallSites.first] = std::map<Instruction *, Function *>();
                    }
                    mapStdRead[CallerCallSites.first][CallInstCallee.first] = CallInstCallee.second;
                } else if (isStdWrite(FuncName)) {
                    if (mapStdWrite.find(CallerCallSites.first) == mapStdWrite.end()) {
                        mapStdWrite[CallerCallSites.first] = std::map<Instruction *, Function *>();
                    }
                    mapStdWrite[CallerCallSites.first][CallInstCallee.first] = CallInstCallee.second;
                }
            }
        }
#ifdef LOCKAPI
{
        std::map<Function *, std::map<Type *, std::map<Instruction *, LockInfo>>> mapIntraProcLockInfo;
        std::unordered_map<MutexSource, std::map<Instruction *, LockInfo>, MutexSourceHasher> mapInterProcLockInfo;
        std::map<Instruction *, std::set<Instruction *>> mapLockDropInst;
        for (auto &CallerCallSites : mapLockAPIRwLockRead) {
            for (auto &CallInstCallee : CallerCallSites.second) {
                // errs() << "Caller: " << CallerCallSites.first->getName() << "\n";
                // errs() << "Callee: " << CallInstCallee.second->getName() << "\n";
                LockInfo LI;
                parseLockAPIRwLockRead(CallInstCallee.first, LI);
                MutexSource MS;
                bool IsField = traceMutexSource(LI.LockValue, MS);
                if (!IsField) {
                    Function *F = LI.LockInst->getFunction();
                    if (mapIntraProcLockInfo.find(F) == mapIntraProcLockInfo.end()) {
                        mapIntraProcLockInfo[F] = std::map<Type *, std::map<Instruction *, LockInfo>>();
                    }
                    Type *LockType = LI.LockValue->getType();
                    if (mapIntraProcLockInfo[F].find(LockType) == mapIntraProcLockInfo[F].end()) {
                        mapIntraProcLockInfo[F][LockType] = std::map<Instruction *, LockInfo>();
                    }
                    mapIntraProcLockInfo[F][LockType][LI.LockInst] = LI;
                } else {
                    if (mapInterProcLockInfo.find(MS) == mapInterProcLockInfo.end()) {
                        mapInterProcLockInfo[MS] = std::map<Instruction *, LockInfo>();
                    }
                    mapInterProcLockInfo[MS][LI.LockInst] = LI;
                }
                std::set<Instruction *> setDropInst;
                traceDropInst(LI, setDropInst);
                mapLockDropInst[LI.LockInst] = setDropInst;
            }
        }

        // for (auto &FTLIS : mapIntraProcLockInfo) {
        //     for (auto &TLIS : FTLIS.second) {
        //         if (TLIS.second.size() > 1) {
        //             errs() << "Set of Aliased Locks:\n";
        //             errs() << FTLIS.first->getName() << "\n";
        //             for (auto &LI : TLIS.second) {
        //                 printDebugInfo(LI.second.LockInst);
        //             }
        //         }
        //     }
        // }

        // for (auto &MSLIS : mapInterProcLockInfo) {
        //     if (MSLIS.second.size() > 1) {
        //         errs() << "Set of Aliased Locks:\n";
        //         MSLIS.first.print(errs());
        //         for (auto &LI : MSLIS.second) {
        //             printDebugInfo(LI.second.LockInst);
        //         }
        //     }
        // }

        // for (auto &LIDIS : mapLockDropInst) {
        //     errs() << "DropInsts for LockInst:";
        //     printDebugInfo(LIDIS.first);
        //     for (Instruction *DI : LIDIS.second) {
        //         printDebugInfo(DI);
        //     }
        // }
        // for (auto &FTLIS : mapIntraProcLockInfo) {
        //     for (auto &TLIS : FTLIS.second) {
        //         if (TLIS.second.size() > 1) {
        //             errs() << "Set of Aliased Locks:\n";
        //             errs() << FTLIS.first->getName() << "\n";
        //             for (auto &LI : TLIS.second) {
        //                 printDebugInfo(LI.second.LockInst);
        //             }
        //         }
        //     }
        // }

        for (auto &FTLIS : mapIntraProcLockInfo) {
            for (auto &TLIS : FTLIS.second) {
                if (TLIS.second.size() <= 1) {
                    continue;
                }
                std::set<Instruction *> setMayAliasLock;
                for (auto &LI : TLIS.second) {
                    setMayAliasLock.insert(LI.first);
                }
                for (auto &LI : TLIS.second) {
                   trackLockInstLocal(LI.first, setMayAliasLock, mapLockDropInst[LI.first]);
                }
            }
        }
// #ifdef INTER
        for (auto &MSLIS : mapInterProcLockInfo) {
            if (MSLIS.second.size() <= 1) {
                continue;
            }
            // errs() << "Set of Aliased Locks:\n";
            // MSLIS.first.print(errs());
            // for (auto &LI : MSLIS.second) {
            //     printDebugInfo(LI.second.LockInst);
            // }
            std::set<Instruction *> setMayAliasLock;
            for (auto &LI : MSLIS.second) {
                setMayAliasLock.insert(LI.first);
            }
            for (auto &LI : MSLIS.second) {
                // if (LI.first->getFunction()->getName() != "_ZN12ethcore_sync10light_sync18LightSync$LT$L$GT$13maintain_sync17h404bd375d3a82a04E") {
                //     continue;
                // } else {
                // errs() << "LockInst:";
                // LI.first->print(errs());
                // errs() << "\n";
                // errs() << "DropInst:\n";
                // for (Instruction *DI : mapLockDropInst[LI.first]) {
                //     DI->print(errs());
                //     errs() << "\n";
                // }
                trackLockInst(LI.first, setMayAliasLock, mapLockDropInst[LI.first], mapGlobalCallSite);
                // break;
                // }
            }
        }
}
#endif  // LOCKAPI
#ifdef STDMUTEX
{
        std::map<Function *, std::map<Type *, std::map<Instruction *, LockInfo>>> mapIntraProcLockInfo;
        std::unordered_map<MutexSource, std::map<Instruction *, LockInfo>, MutexSourceHasher> mapInterProcLockInfo;
        std::map<Instruction *, std::set<Instruction *>> mapLockDropInst;
        for (auto &CallerCallSites : mapStdLock) {
            for (auto &CallInstCallee : CallerCallSites.second) {
                // errs() << "Caller: " << CallerCallSites.first->getName() << "\n";
                // errs() << "Callee: " << CallInstCallee.second->getName() << "\n";
                LockInfo LI;
                parseStdLockWrite(CallInstCallee.first, LI);
                MutexSource MS;
                bool IsField = traceMutexSource(LI.LockValue, MS);
                if (!IsField) {
                    Function *F = LI.LockInst->getFunction();
                    if (mapIntraProcLockInfo.find(F) == mapIntraProcLockInfo.end()) {
                        mapIntraProcLockInfo[F] = std::map<Type *, std::map<Instruction *, LockInfo>>();
                    }
                    Type *LockType = LI.LockValue->getType();
                    if (mapIntraProcLockInfo[F].find(LockType) == mapIntraProcLockInfo[F].end()) {
                        mapIntraProcLockInfo[F][LockType] = std::map<Instruction *, LockInfo>();
                    }
                    mapIntraProcLockInfo[F][LockType][LI.LockInst] = LI;
                } else {
                    if (mapInterProcLockInfo.find(MS) == mapInterProcLockInfo.end()) {
                        mapInterProcLockInfo[MS] = std::map<Instruction *, LockInfo>();
                    }
                    mapInterProcLockInfo[MS][LI.LockInst] = LI;
                }
                std::set<Instruction *> setDropInst;
                traceResult(LI, setDropInst, M.getDataLayout());
                //errs() << "setDropInst\n";
                //for (Instruction *DI : setDropInst) {
                //    DI->print(errs());
                //    errs() << "\n";
                //}
                mapLockDropInst[LI.LockInst] = setDropInst;
            }
        }

        // for (auto &FTLIS : mapIntraProcLockInfo) {
        //     for (auto &TLIS : FTLIS.second) {
        //         if (TLIS.second.size() > 1) {
        //             errs() << "Set of Aliased Locks:\n";
        //             errs() << FTLIS.first->getName() << "\n";
        //             for (auto &LI : TLIS.second) {
        //                 printDebugInfo(LI.second.LockInst);
        //             }
        //         }
        //     }
        // }

        // for (auto &MSLIS : mapInterProcLockInfo) {
        //     if (MSLIS.second.size() > 1) {
        //         errs() << "Set of Aliased Locks:\n";
        //         MSLIS.first.print(errs());
        //         for (auto &LI : MSLIS.second) {
        //             printDebugInfo(LI.second.LockInst);
        //         }
        //     }
        // }

        // for (auto &LIDIS : mapLockDropInst) {
        //     errs() << "DropInsts for LockInst:";
        //     printDebugInfo(LIDIS.first);
        //     for (Instruction *DI : LIDIS.second) {
        //         printDebugInfo(DI);
        //     }
        // }
        // for (auto &FTLIS : mapIntraProcLockInfo) {
        //     for (auto &TLIS : FTLIS.second) {
        //         if (TLIS.second.size() > 1) {
        //             errs() << "Set of Aliased Locks:\n";
        //             errs() << FTLIS.first->getName() << "\n";
        //             for (auto &LI : TLIS.second) {
        //                 printDebugInfo(LI.second.LockInst);
        //             }
        //         }
        //     }
        // }
//#ifdef INTRA
        for (auto &FTLIS : mapIntraProcLockInfo) {
            for (auto &TLIS : FTLIS.second) {
                if (TLIS.second.size() <= 1) {
                    continue;
                }
                for (auto &LI : TLIS.second) {
                //errs() << "LockInst:";
                //LI.first->print(errs());
                //errs() << "\n";
                //errs() << "DropInst:\n";
                //for (Instruction *DI : mapLockDropInst[LI.first]) {
                //    DI->print(errs());
                //    errs() << "\n";
                //}
                   Function *MyFunc = FTLIS.first;
                   AliasAnalysis &AA = getAnalysis<AAResultsWrapperPass>(*MyFunc).getAAResults();
                   std::set<Instruction *> setMayAliasLock;
                   for (auto &LI2 : TLIS.second) {
                      if (LI.first == LI2.first) {
                          continue;
                      }
                      if (AA.alias(LI.first, LI2.first) == AliasResult::MustAlias) {
                          setMayAliasLock.insert(LI2.first);
                      }
                   }
                   trackLockInstLocal(LI.first, setMayAliasLock, mapLockDropInst[LI.first]);
                }
            }
        }
//#endif // INTRA
//#ifdef INTER
        for (auto &MSLIS : mapInterProcLockInfo) {
            if (MSLIS.second.size() <= 1) {
                continue;
            }
            // errs() << "Set of Aliased Locks:\n";
            // MSLIS.first.print(errs());
            // for (auto &LI : MSLIS.second) {
            //     printDebugInfo(LI.second.LockInst);
            // }
            std::set<Instruction *> setMayAliasLock;
            for (auto &LI : MSLIS.second) {
                setMayAliasLock.insert(LI.first);
            }
            for (auto &LI : MSLIS.second) {
                // if (LI.first->getFunction()->getName() != "_ZN12ethcore_sync10light_sync18LightSync$LT$L$GT$13maintain_sync17h404bd375d3a82a04E") {
                //     continue;
                // } else {
                //errs() << "LockInst:";
                //LI.first->print(errs());
                //errs() << "\n";
                //errs() << "DropInst:\n";
                //for (Instruction *DI : mapLockDropInst[LI.first]) {
                //    DI->print(errs());
                //    errs() << "\n";
                //}
                trackLockInst(LI.first, setMayAliasLock, mapLockDropInst[LI.first], mapGlobalCallSite);
                // break;
                // }
            }
        }
//#endif
}
#endif // STDMUTEX

#ifdef STDRWLOCK
{
        std::map<Function *, std::map<Type *, std::map<Instruction *, LockInfo>>> mapIntraProcLockInfo;
        std::unordered_map<MutexSource, std::map<Instruction *, LockInfo>, MutexSourceHasher> mapInterProcLockInfo;
        std::map<Instruction *, std::set<Instruction *>> mapLockDropInst;
        //for (auto &CallerCallSites : mapStdRead) {
        //    for (auto &CallInstCallee : CallerCallSites.second) {
        //        // errs() << "Caller: " << CallerCallSites.first->getName() << "\n";
        //        // errs() << "Callee: " << CallInstCallee.second->getName() << "\n";
        //        LockInfo LI;
        //        parseStdRead(CallInstCallee.first, LI);
        //        MutexSource MS;
        //        bool IsField = traceMutexSource(LI.LockValue, MS);
        //        if (!IsField) {
        //            Function *F = LI.LockInst->getFunction();
        //            if (mapIntraProcLockInfo.find(F) == mapIntraProcLockInfo.end()) {
        //                mapIntraProcLockInfo[F] = std::map<Type *, std::map<Instruction *, LockInfo>>();
        //            }
        //            Type *LockType = LI.LockValue->getType();
        //            if (mapIntraProcLockInfo[F].find(LockType) == mapIntraProcLockInfo[F].end()) {
        //                mapIntraProcLockInfo[F][LockType] = std::map<Instruction *, LockInfo>();
        //            }
        //            mapIntraProcLockInfo[F][LockType][LI.LockInst] = LI;
        //        } else {
        //            if (mapInterProcLockInfo.find(MS) == mapInterProcLockInfo.end()) {
        //                mapInterProcLockInfo[MS] = std::map<Instruction *, LockInfo>();
        //            }
        //            mapInterProcLockInfo[MS][LI.LockInst] = LI;
        //        }
        //        std::set<Instruction *> setDropInst;
        //        traceResult(LI, setDropInst, M.getDataLayout());
        //        mapLockDropInst[LI.LockInst] = setDropInst;
        //    }
        //}

        for (auto &CallerCallSites : mapStdWrite) {
            for (auto &CallInstCallee : CallerCallSites.second) {
                // errs() << "Caller: " << CallerCallSites.first->getName() << "\n";
                // errs() << "Callee: " << CallInstCallee.second->getName() << "\n";
                LockInfo LI;
                parseStdLockWrite(CallInstCallee.first, LI);
                MutexSource MS;
                bool IsField = traceMutexSource(LI.LockValue, MS);
                if (!IsField) {
                    Function *F = LI.LockInst->getFunction();
                    if (mapIntraProcLockInfo.find(F) == mapIntraProcLockInfo.end()) {
                        mapIntraProcLockInfo[F] = std::map<Type *, std::map<Instruction *, LockInfo>>();
                    }
                    Type *LockType = LI.LockValue->getType();
                    if (mapIntraProcLockInfo[F].find(LockType) == mapIntraProcLockInfo[F].end()) {
                        mapIntraProcLockInfo[F][LockType] = std::map<Instruction *, LockInfo>();
                    }
                    mapIntraProcLockInfo[F][LockType][LI.LockInst] = LI;
                } else {
                    if (mapInterProcLockInfo.find(MS) == mapInterProcLockInfo.end()) {
                        mapInterProcLockInfo[MS] = std::map<Instruction *, LockInfo>();
                    }
                    mapInterProcLockInfo[MS][LI.LockInst] = LI;
                }
                //errs() << "LockInst:\n";
                //errs() << LI.LockInst->getParent()->getName() << ": ";
                //LI.LockInst->print(errs());
                //errs() << "\n";
                std::set<Instruction *> setDropInst;
                traceResult(LI, setDropInst, M.getDataLayout());
                //for (Instruction *DI : setDropInst) {
                //    errs() << DI->getParent()->getName() << ": ";
                //    DI->print(errs());
                //    errs() << "\n";
                //}
                mapLockDropInst[LI.LockInst] = setDropInst;
            }
        }

        // for (auto &FTLIS : mapIntraProcLockInfo) {
        //     for (auto &TLIS : FTLIS.second) {
        //         if (TLIS.second.size() > 1) {
        //             errs() << "Set of Aliased Locks:\n";
        //             errs() << FTLIS.first->getName() << "\n";
        //             for (auto &LI : TLIS.second) {
        //                 printDebugInfo(LI.second.LockInst);
        //             }
        //         }
        //     }
        // }

        // for (auto &MSLIS : mapInterProcLockInfo) {
        //     if (MSLIS.second.size() > 1) {
        //         errs() << "Set of Aliased Locks:\n";
        //         MSLIS.first.print(errs());
        //         for (auto &LI : MSLIS.second) {
        //             printDebugInfo(LI.second.LockInst);
        //         }
        //     }
        // }

        // for (auto &LIDIS : mapLockDropInst) {
        //     errs() << "DropInsts for LockInst:";
        //     printDebugInfo(LIDIS.first);
        //     for (Instruction *DI : LIDIS.second) {
        //         printDebugInfo(DI);
        //     }
        // }
        // for (auto &FTLIS : mapIntraProcLockInfo) {
        //     for (auto &TLIS : FTLIS.second) {
        //         if (TLIS.second.size() > 1) {
        //             errs() << "Set of Aliased Locks:\n";
        //             errs() << FTLIS.first->getName() << "\n";
        //             for (auto &LI : TLIS.second) {
        //                 printDebugInfo(LI.second.LockInst);
        //             }
        //         }
        //     }
        // }
#ifdef INTRA
        for (auto &FTLIS : mapIntraProcLockInfo) {
            for (auto &TLIS : FTLIS.second) {
                if (TLIS.second.size() <= 1) {
                    continue;
                }
                std::set<Instruction *> setMayAliasLock;
                for (auto &LI : TLIS.second) {
                    setMayAliasLock.insert(LI.first);
                }
                for (auto &LI : TLIS.second) {
                   trackLockInstLocal(LI.first, setMayAliasLock, mapLockDropInst[LI.first]);
                }
            }
        }
#endif
// #ifdef INTER
        for (auto &MSLIS : mapInterProcLockInfo) {
            if (MSLIS.second.size() <= 1) {
                continue;
            }
            // errs() << "Set of Aliased Locks:\n";
            // MSLIS.first.print(errs());
            // for (auto &LI : MSLIS.second) {
            //     printDebugInfo(LI.second.LockInst);
            // }
            std::set<Instruction *> setMayAliasLock;
            for (auto &LI : MSLIS.second) {
                setMayAliasLock.insert(LI.first);
            }
            for (auto &LI : MSLIS.second) {
                
                // if (LI.first->getFunction()->getName() != "_ZN12ethcore_sync10light_sync18LightSync$LT$L$GT$13maintain_sync17h404bd375d3a82a04E") {
                //     continue;
                // } else {
                // errs() << "LockInst:";
                // LI.first->print(errs());
                // errs() << "\n";
                // errs() << "DropInst:\n";
                // for (Instruction *DI : mapLockDropInst[LI.first]) {
                //     DI->print(errs());
                //     errs() << "\n";
                // }
                trackLockInst(LI.first, setMayAliasLock, mapLockDropInst[LI.first], mapGlobalCallSite);
                // break;
                // }
            }
        }

}
#endif // STDRWLOCK
        return false;
    }

}  // namespace detector

static RegisterPass<detector::RustDoubleLockDetector> X(
        "detect",
        "Detect Double Lock",
        false,
        true);
