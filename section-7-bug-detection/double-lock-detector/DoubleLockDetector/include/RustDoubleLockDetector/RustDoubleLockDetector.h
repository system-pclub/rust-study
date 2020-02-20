#ifndef RUSTBUGDETECTOR_RUSTDOUBLELOCKDETECTOR_H
#define RUSTBUGDETECTOR_RUSTDOUBLELOCKDETECTOR_H

#include "llvm/Pass.h"

namespace detector {
    struct RustDoubleLockDetector : public llvm::ModulePass {

        static char ID;

        RustDoubleLockDetector();

        void getAnalysisUsage(llvm::AnalysisUsage &AU) const override;

        bool runOnModule(llvm::Module &M) override;

    private:

        llvm::Module *pModule;
    };
}

#endif //RUSTBUGDETECTOR_RustDOUBLELOCKDETECTOR_H
