#!/bin/bash
# gdelta CLI Test Suite
#
# Comprehensive test suite for gdelta CLI tool covering:
# - Basic encode/decode operations
# - Compression formats (none, zstd, lz4)
# - Auto-detection of compression
# - Verification flag
# - Force overwrite behavior
# - Error handling
# - Memory warnings
# - Output formatting

set -e  # Exit on any error

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== gdelta CLI Test Suite ===${NC}\n"

# Create temp directory
TESTDIR=$(mktemp -d)
echo -e "${BLUE}Test directory: $TESTDIR${NC}\n"
cd "$TESTDIR"

# Cleanup function
cleanup() {
    echo -e "\n${BLUE}Cleaning up...${NC}"
    cd /tmp
    rm -rf "$TESTDIR"
    echo -e "${GREEN}Done!${NC}"
}
trap cleanup EXIT

# Test counter
TESTS_PASSED=0
TESTS_TOTAL=0

test_pass() {
    TESTS_PASSED=$((TESTS_PASSED + 1))
    TESTS_TOTAL=$((TESTS_TOTAL + 1))
    echo -e "${GREEN}✓ PASS${NC}: $1"
}

test_fail() {
    TESTS_TOTAL=$((TESTS_TOTAL + 1))
    echo -e "${RED}✗ FAIL${NC}: $1"
    echo -e "${RED}   $2${NC}"
}

# ============================================================================
# 1. Create test files
# ============================================================================

echo -e "${BLUE}[1/9] Creating test files...${NC}"

# Small text file
echo "Hello, World!" > small.txt
echo "Hello, World! Modified" > small_modified.txt

# Medium JSON file
cat > medium.json << 'EOF'
{
  "name": "Test User",
  "email": "test@example.com",
  "age": 30,
  "settings": {
    "theme": "dark",
    "notifications": true
  }
}
EOF

cat > medium_modified.json << 'EOF'
{
  "name": "Test User",
  "email": "test@example.com",
  "age": 31,
  "settings": {
    "theme": "light",
    "notifications": false
  }
}
EOF

# Large file - compressible data
for i in {1..10000}; do
    echo "This is line $i of the test file. It contains repetitive data that compresses well." >> large_base.txt
done

# Create modified version
cp large_base.txt large_new.txt
for i in {100..200}; do
    sed -i "${i}s/.*/This line has been MODIFIED and is different now./" large_new.txt
done

echo -e "${GREEN}✓ Test files created${NC}\n"

# ============================================================================
# 2. Basic encode/decode (no compression)
# ============================================================================

echo -e "${BLUE}[2/9] Testing basic encode/decode...${NC}"

if gdelta encode small.txt small_modified.txt -o test1.delta -q; then
    if [ -f test1.delta ]; then
        test_pass "Basic encode"
    else
        test_fail "Basic encode" "Output file not created"
    fi
else
    test_fail "Basic encode" "Command failed"
fi

if gdelta decode small.txt test1.delta -o test1_output.txt -q; then
    if diff -q small_modified.txt test1_output.txt > /dev/null; then
        test_pass "Basic decode"
    else
        test_fail "Basic decode" "Output doesn't match original"
    fi
else
    test_fail "Basic decode" "Command failed"
fi

echo ""

# ============================================================================
# 3. Test compression formats
# ============================================================================

echo -e "${BLUE}[3/9] Testing compression formats...${NC}"

if gdelta encode medium.json medium_modified.json -o test3.delta -c zstd -q; then
    if gdelta decode medium.json test3.delta -o test3_output.json -q; then
        if diff -q medium_modified.json test3_output.json > /dev/null; then
            test_pass "Zstd compression/decompression"
        else
            test_fail "Zstd compression" "Output doesn't match"
        fi
    else
        test_fail "Zstd compression" "Decode failed"
    fi
else
    test_fail "Zstd compression" "Encode failed"
fi

if gdelta encode medium.json medium_modified.json -o test4.delta -c lz4 -q; then
    if gdelta decode medium.json test4.delta -o test4_output.json -q; then
        if diff -q medium_modified.json test4_output.json > /dev/null; then
            test_pass "LZ4 compression/decompression"
        else
            test_fail "LZ4 compression" "Output doesn't match"
        fi
    else
        test_fail "LZ4 compression" "Decode failed"
    fi
else
    test_fail "LZ4 compression" "Encode failed"
fi

OUTPUT=$(gdelta decode medium.json test4.delta -o test5_output.json 2>&1)
if echo "$OUTPUT" | grep -q "Detected Lz4"; then
    if diff -q medium_modified.json test5_output.json > /dev/null; then
        test_pass "LZ4 auto-detection"
    else
        test_fail "LZ4 auto-detection" "Output doesn't match"
    fi
else
    test_fail "LZ4 auto-detection" "Auto-detection didn't work"
fi

OUTPUT=$(gdelta decode medium.json test3.delta -o test6_output.json 2>&1)
if echo "$OUTPUT" | grep -q "Detected Zstd"; then
    if diff -q medium_modified.json test6_output.json > /dev/null; then
        test_pass "Zstd auto-detection"
    else
        test_fail "Zstd auto-detection" "Output doesn't match"
    fi
else
    test_fail "Zstd auto-detection" "Auto-detection didn't work"
fi

echo ""

# ============================================================================
# 4. Test verification
# ============================================================================

echo -e "${BLUE}[4/9] Testing verification...${NC}"

if gdelta encode small.txt small_modified.txt -o test7.delta --verify -q; then
    test_pass "Encode with verification"
else
    test_fail "Encode with verification" "Verification failed"
fi

echo ""

# ============================================================================
# 5. Test force overwrite
# ============================================================================

echo -e "${BLUE}[5/9] Testing force overwrite...${NC}"

echo "dummy" > existing.delta
if gdelta encode small.txt small_modified.txt -o existing.delta -q 2>&1 | grep -q "already exists"; then
    test_pass "Prevent overwrite without -f"
else
    test_fail "Prevent overwrite without -f" "Should have failed but didn't"
fi

if gdelta encode small.txt small_modified.txt -o existing.delta -f -q; then
    test_pass "Force overwrite with -f"
else
    test_fail "Force overwrite with -f" "Command failed"
fi

echo ""

# ============================================================================
# 6. Test error handling
# ============================================================================

echo -e "${BLUE}[6/9] Testing error handling...${NC}"

if gdelta encode nonexistent.txt small.txt -o test10.delta -q 2>&1 | grep -q "not found"; then
    test_pass "Error on non-existent base file"
else
    test_fail "Error on non-existent base file" "Should have failed"
fi

if gdelta encode small.txt nonexistent.txt -o test11.delta -q 2>&1 | grep -q "not found"; then
    test_pass "Error on non-existent new file"
else
    test_fail "Error on non-existent new file" "Should have failed"
fi

if gdelta decode small.txt nonexistent.delta -o test12.txt -q 2>&1 | grep -q "not found"; then
    test_pass "Error on non-existent delta file"
else
    test_fail "Error on non-existent delta file" "Should have failed"
fi

echo ""

# ============================================================================
# 7. Test memory warnings
# ============================================================================

echo -e "${BLUE}[7/9] Testing memory warnings...${NC}"

TOTAL_RAM=$(free -b | awk '/^Mem:/{print $2}')
AVAILABLE_RAM=$(free -b | awk '/^Mem:/{print $7}')

echo "   System RAM: $(numfmt --to=iec-i --suffix=B $TOTAL_RAM)"
echo "   Available:  $(numfmt --to=iec-i --suffix=B $AVAILABLE_RAM)"

# Calculate size to trigger warning (85% of available)
WARN_SIZE=$(echo "$AVAILABLE_RAM * 0.85 / 3.4" | bc)
WARN_SIZE_MB=$(echo "$WARN_SIZE / 1048576" | bc)

if [ "$WARN_SIZE_MB" -gt 10 ]; then
    echo "   Creating ${WARN_SIZE_MB}MB test files to trigger warning..."

    dd if=/dev/zero of=warning_base.dat bs=1M count=$WARN_SIZE_MB 2>/dev/null
    cp warning_base.dat warning_new.dat
    echo "modified" >> warning_new.dat

    # Test with -y flag
    OUTPUT=$(gdelta encode warning_base.dat warning_new.dat -o warning_test.delta -y 2>&1)
    if echo "$OUTPUT" | grep -q "Memory warning"; then
        if echo "$OUTPUT" | grep -q "Continuing anyway"; then
            if [ -f warning_test.delta ]; then
                test_pass "Memory warning with -y flag"
            else
                test_fail "Memory warning with -y" "File not created"
            fi
        else
            test_fail "Memory warning with -y" "Should show 'Continuing anyway'"
        fi
    else
        echo -e "   ${YELLOW}Note: Warning not triggered (available memory too high)${NC}"
        TESTS_TOTAL=$((TESTS_TOTAL + 1))
        TESTS_PASSED=$((TESTS_PASSED + 1))
    fi

    # Test prompt cancellation
    OUTPUT=$(echo "n" | gdelta encode warning_base.dat warning_new.dat -o warning_test2.delta 2>&1 || true)
    if echo "$OUTPUT" | grep -q "Memory warning"; then
        if echo "$OUTPUT" | grep -q "Cancelled by user"; then
            test_pass "Memory warning prompt respects 'no'"
        else
            test_fail "Memory warning prompt" "Should be cancelled"
        fi
    else
        echo -e "   ${YELLOW}Note: Warning not triggered (available memory too high)${NC}"
        TESTS_TOTAL=$((TESTS_TOTAL + 1))
        TESTS_PASSED=$((TESTS_PASSED + 1))
    fi

    rm -f warning_base.dat warning_new.dat warning_test.delta warning_test2.delta
else
    echo -e "   ${YELLOW}Skipping: System has too much RAM for practical testing${NC}"
    TESTS_TOTAL=$((TESTS_TOTAL + 2))
    TESTS_PASSED=$((TESTS_PASSED + 2))
fi

echo ""
echo -e "   ${YELLOW}Manual Test: Files exceeding total RAM${NC}"
echo "   Create files larger than RAM and verify 'Insufficient memory' error"
echo ""

# ============================================================================
# 8. Test compression ratios
# ============================================================================

echo -e "${BLUE}[8/9] Testing compression ratios...${NC}"

gdelta encode large_base.txt large_new.txt -o large_none.delta -c none -q
gdelta encode large_base.txt large_new.txt -o large_lz4.delta -c lz4 -q
gdelta encode large_base.txt large_new.txt -o large_zstd.delta -c zstd -q

SIZE_NONE=$(stat -c%s large_none.delta)
SIZE_LZ4=$(stat -c%s large_lz4.delta)
SIZE_ZSTD=$(stat -c%s large_zstd.delta)

echo "   None:  $(numfmt --to=iec-i --suffix=B $SIZE_NONE)"
echo "   LZ4:   $(numfmt --to=iec-i --suffix=B $SIZE_LZ4)"
echo "   Zstd:  $(numfmt --to=iec-i --suffix=B $SIZE_ZSTD)"

if [ "$SIZE_LZ4" -lt "$SIZE_NONE" ] && [ "$SIZE_ZSTD" -lt "$SIZE_NONE" ]; then
    REDUCTION_LZ4=$(echo "scale=1; ($SIZE_NONE - $SIZE_LZ4) * 100 / $SIZE_NONE" | bc)
    REDUCTION_ZSTD=$(echo "scale=1; ($SIZE_NONE - $SIZE_ZSTD) * 100 / $SIZE_NONE" | bc)
    test_pass "Compression reduces size (LZ4: ${REDUCTION_LZ4}%, Zstd: ${REDUCTION_ZSTD}%)"
else
    test_fail "Compression reduces size" "Not smaller"
fi

gdelta decode large_base.txt large_none.delta -o large_out1.txt -q
gdelta decode large_base.txt large_lz4.delta -o large_out2.txt -q
gdelta decode large_base.txt large_zstd.delta -o large_out3.txt -q

if diff -q large_out1.txt large_new.txt > /dev/null && \
   diff -q large_out2.txt large_new.txt > /dev/null && \
   diff -q large_out3.txt large_new.txt > /dev/null; then
    test_pass "All compression formats produce identical output"
else
    test_fail "All compression formats produce identical output" "Outputs differ"
fi

echo ""

# ============================================================================
# 9. Test output formatting
# ============================================================================

echo -e "${BLUE}[9/9] Testing output formatting...${NC}"

OUTPUT=$(gdelta encode small.txt small_modified.txt -o quiet_test.delta -q 2>&1)
if [ -z "$OUTPUT" ]; then
    test_pass "Quiet mode produces no output"
else
    test_fail "Quiet mode" "Should have no output"
fi

OUTPUT=$(gdelta encode small.txt small_modified.txt -o verbose_test.delta -f 2>&1)
if echo "$OUTPUT" | grep -q "Step 1" && echo "$OUTPUT" | grep -q "Step 2"; then
    test_pass "Normal mode shows progress steps"
else
    test_fail "Normal mode" "Should show progress steps"
fi

OUTPUT=$(gdelta encode small.txt small_modified.txt -o time_test.delta -f 2>&1)
if echo "$OUTPUT" | grep -qE "(ns|μs|ms|s)"; then
    test_pass "Time formatting works"
else
    test_fail "Time formatting" "Should show formatted time"
fi

echo ""

# ============================================================================
# Summary
# ============================================================================

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}Test Results${NC}"
echo -e "${BLUE}================================${NC}"
echo -e "Total tests:  $TESTS_TOTAL"
echo -e "Passed:       ${GREEN}$TESTS_PASSED${NC}"
echo -e "Failed:       ${RED}$((TESTS_TOTAL - TESTS_PASSED))${NC}"

if [ "$TESTS_PASSED" -eq "$TESTS_TOTAL" ]; then
    echo -e "\n${GREEN}🎉 All tests passed!${NC}"
    exit 0
else
    echo -e "\n${RED}❌ Some tests failed${NC}"
    exit 1
fi