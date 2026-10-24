# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

import unittest
import numpy as np
from fit_early import fit_early


class FitTests(unittest.TestCase):
    def basis(self):
        random=np.random.default_rng(19)
        early=random.normal(size=(8000,2))
        early[320:]=0
        late=random.normal(size=(8000,2))
        late[:800]=0
        late*=np.exp(-np.arange(8000)[:,None]/1600)
        def stereo(ms,width):
            return np.stack([ms[:,0]+width*ms[:,1],ms[:,0]-width*ms[:,1]],axis=1)
        return stereo(late,.8),stereo(late+early,.8),stereo(late+.45*early,.7)

    def test_recovers_known_balance_and_width_from_independent_bases(self):
        late,unit,reference=self.basis()
        result=fit_early(reference,late,unit,8000,.8,0.)
        self.assertAlmostEqual(result['early_level'],.45,places=5)
        self.assertAlmostEqual(result['width'],.7,places=5)

    def test_rejects_silence_missing_basis_and_invalid_dimensions(self):
        late,unit,reference=self.basis()
        for ref,a,b in [(reference,late,late),(reference,late[:-1],unit),(reference[:,0],late,unit),
                        (reference*0,late,unit)]:
            with self.assertRaises(ValueError): fit_early(ref,a,b,8000,.8,0.)
        for width in [0.,-1.,float('nan'),2.]:
            with self.assertRaises(ValueError): fit_early(reference,late,unit,8000,width,0.)


if __name__=='__main__': unittest.main()
