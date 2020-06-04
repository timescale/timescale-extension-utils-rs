#!/bin/bash

set -eu -o pipefail

PGCONFIG=$1
shift

PGMAJOR=`"$PGCONFIG" --version`
PGMAJOR=${PGMAJOR#"PostgreSQL "}
PGMAJOR=${PGMAJOR%%.*}

INCLUDEDIR=`"$PGCONFIG" --includedir-server`

bindgen $@ -- -I"$INCLUDEDIR"
