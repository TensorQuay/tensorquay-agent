# Workstation data libraries: licence audit (F24)

pandas, matplotlib and openpyxl come from Debian into the Workstation image, installed with `--no-install-recommends`
from the pinned snapshot in `Dockerfile`. The Product Owner decided this route on 30 Sep 2026 (licence option A): the
image is an aggregate of separately licensed Debian packages (ADR-0017 point 7), and each release publishes its exact
package list with every package's licence, copyright file and source location in `PACKAGES.md` beside this file
(generated from the built image by `packages.py`).

Below is the audit made before that decision, of the packages these libraries brought in with their dependencies
(recommends included, so it names a few the image doesn't install), as `package|licences from its Debian copyright
file`. Six fall outside the permissive-only rule the web and Rust dependencies follow: fonts-lyx, libgcrypt20,
libgpg-error0, libimagequant0, libgfortran5, and python-is-python3, which isn't installed (`python` is a plain link).

```text
blt|MIT-1;MIT-2;MIT-3;MIT-4;MIT-5;MIT-6;Tcl;
fonts-dejavu-core|GPL-2+;bitstream-vera;
fonts-dejavu-mono|GPL-2+;bitstream-vera;
fonts-lyx|BSD-2-Mark-Modifications;BSD-3-Clause;BSL-1.0;BaKoMa;CC0-1.0;GPL-2+;LGPL-2.1+;public-domain;
libblas3|BSD-3-clause;BSD-3-clause-intel;
libgcrypt20|;
libgfortran5|(not machine-readable: BSD,GNU General Public License,GNU Library General Public License,GPL,LGPL,MIT,Public domain,bsd,mit,)
libgpg-error0|BSD-3-clause;GPL-3+;LGPL-2.1+;LGPL-2.1+ or BSD-3-clause;g10-permissive;
libimagequant0|CC0;GPL-3.0+;MIT;
libjs-jquery|Expat;
libjs-jquery-ui|CC-BY-SA-3.0;CC0;Expat;GPL-2;GPL-2 or Expat;
liblapack3|BSD-3-clause;BSD-3-clause-intel;
libqhull-r8.0|GPL-3+;Qhull;
libraqm0|MIT;
libtcl8.6|(not machine-readable: BSD,MIT,mit,)
libtk8.6|(not machine-readable: MIT,)
libwebpdemux2|Apache-2.0;BSD-3-Clause;
libwebpmux3|Apache-2.0;BSD-3-Clause;
libxslt1.1|(not machine-readable: MIT,mit,)
libxss1|(not machine-readable: MIT,mit,)
libzopfli1|Apache-2.0;GPL-3+;Zlib;
python-is-python3|GPL-3.0;
python-matplotlib-data|ANTIGRAIN;Adobe-Fonts;BSD;BSD-3-Clause;CC-BY-4.0;Custom-BAKOMA;Custom-DEJAVU;Custom-PSF-compatible;Custom-STIX;Custom-Tcl-Tk;Custom-knuth;Custom-ppr;Matplotlib-License;
python3-attr|Expat;
python3-brotli|MIT;
python3-contourpy|BSD-3-Clause;
python3-cycler|BSD;
python3-dateutil|BSD-3-clause;
python3-decorator|BSD-2-clause;
python3-et-xmlfile|MIT;
python3-fonttools|Apache-2.0;BSD-3-Clause-Adobe;Expat;GPL-2 with font exception;GPL-2 with font exception or OFL-1.1;OFL-1.1;X11-Unicode;
python3-fs|BSD-3-clause;Expat;
python3-kiwisolver|;BSD-3-Clause;
python3-lxml|;GPL2 or later;
python3-lz4|BSD-3-clause;GPL-2+;GPL-2+ or BSD-3-clause;
python3-matplotlib|ANTIGRAIN;Adobe-Fonts;BSD;BSD-3-Clause;CC-BY-4.0;Custom-BAKOMA;Custom-DEJAVU;Custom-PSF-compatible;Custom-STIX;Custom-Tcl-Tk;Custom-knuth;Custom-ppr;Matplotlib-License;
python3-mpmath|BSD-3-clause;
python3-numpy|Apache-2;Apache-2 or BSD-3-Clause;BSD-3-Clause;BSD-3-clause or Expat;CC0;Expat;FSFAP;ZLib;Zlib;
python3-numpy-dev|Apache-2;Apache-2 or BSD-3-Clause;BSD-3-Clause;BSD-3-clause or Expat;CC0;Expat;FSFAP;ZLib;Zlib;
python3-openpyxl|MIT;
python3-pandas|Apache-2.0;Apache-2.0 and BSD-3;Apache-2.0 or BSD-3;BSD-2;BSD-3;BSD-3 and Expat;CC-BY-SA-3.0;Expat;Expat and BSD-3;Python;Python and BSD-3;public-domain;
python3-pandas-lib|Apache-2.0;Apache-2.0 and BSD-3;Apache-2.0 or BSD-3;BSD-2;BSD-3;BSD-3 and Expat;CC-BY-SA-3.0;Expat;Expat and BSD-3;Python;Python and BSD-3;public-domain;
python3-pil|(not machine-readable: Public domain,)
python3-pil.imagetk|(not machine-readable: Public domain,)
python3-platformdirs|Expat;
python3-pyparsing|BSD-2-clause;BSD-3-clause;Expat;MIT/X11;ellis-and-grant;salvolainen;
python3-pytz|Expat;
python3-scipy|Apache-2.0;Apache-2.0+MIT;BSD-2-clause;BSD-3-Clause;BSD-3-clause;BSD-LBNL;Boost-1.0;Expat;MIT;PSF-2;bsdlike;expat;qhullbsd;
python3-sympy|BSD-3-clause;GPL-2+;
python3-tk|;
python3-ufolib2|Apache-2.0;Expat;
python3-unicodedata2|Apache-2.0;PSF-2.0;
python3-zopfli|Apache-2.0;Apache-2.0 or CC0-1.0;CC0-1.0;
python3.13-tk|;* Permission to use this software in any way is granted without;By obtaining, using, and/or copying this software and/or its;Permission  is  hereby granted,  free  of charge,  to  any person;Permission is hereby granted, free of charge, to any person obtaining;Permission to use, copy, modify, and distribute this software and;Permission to use, copy, modify, and distribute this software and its;Permission to use, copy, modify, and distribute this software for any;Redistribution and use in source and binary forms, with or without;This software is provided 'as-is', without any express or implied;This software is provided as-is, without express or implied;
tk8.6-blt2.5|MIT-1;MIT-2;MIT-3;MIT-4;MIT-5;MIT-6;Tcl;
unicode-data|(not machine-readable: MIT,mit,)
```
