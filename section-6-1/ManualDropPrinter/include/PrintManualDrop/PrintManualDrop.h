#ifndef RUSTBUGDETECTOR_PRINTMANUALDROP_H
#define RUSTBUGDETECTOR_PRINTMANUALDROP_H

#include "llvm/Pass.h"

namespace detector {

    struct PrintManualDrop : llvm::ModulePass {

        static char ID;

        PrintManualDrop();

        void getAnalysisUsage(llvm::AnalysisUsage &AU) const override;

        bool runOnModule(llvm::Module &M) override;

    private:

        llvm::Module *pModule;
    };
}


#endif //RUSTBUGDETECTOR_PRINTMANUALDROP_H
