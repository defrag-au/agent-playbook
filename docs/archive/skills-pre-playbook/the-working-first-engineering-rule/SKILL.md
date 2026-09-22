---
name: the-working-first-engineering-rule
description: (no description)
disable-model-invocation: true
---

## 📜 **Core Principle**

> **"Always deliver working functionality before optimizing architecture. Fix problems, don't hide them. Prove concepts before perfecting them."**

## 🛡️ **Mandatory Decision Framework**

When facing any compilation error, architectural challenge, or complex implementation:

### **Step 1: STOP and Ask**
- **"Can I make this work in the simplest possible way first?"**
- **"Am I trying to solve too many problems at once?"**
- **"What's the minimal change that proves this concept works?"**

### **Step 2: CHOOSE Path**
**✅ Senior Path: FIX & PROVE**
- Identify root cause of the issue
- Implement minimal working solution
- Verify end-to-end functionality
- *Then* iterate and improve

**❌ Junior Path: HIDE & PERFECT** 
- Comment out problematic code
- Fight complex architectural issues
- Optimize before proving it works
- Prioritize builds over functionality

### **Step 3: VALIDATE**
- **"Does this actually work for the user?"**
- **"Can I test this end-to-end right now?"**
- **"Have I proven the concept or just made it compile?"**

## 🚨 **Red Flag Triggers**

**Immediately STOP and reconsider if I catch myself:**
- Adding `// TODO:` comments to disable functionality
- Commenting out code to fix compilation errors
- Saying "we'll implement this later" for core features
- Fighting async/lifetime issues before proving basic logic works
- Spending more time on architecture than on working functionality

## ✅ **Green Light Indicators**

**Continue confidently when:**
- User can test the feature end-to-end
- Core functionality works, even if simple
- I can demonstrate value immediately
- Each change maintains or improves working state
- Problems are being solved, not hidden

## 📋 **Implementation Mantra**

**"Make it work, make it right, make it fast - in that order."**

1. **Make it work**: Minimal viable implementation that users can test
2. **Make it right**: Proper architecture, error handling, optimization  
3. **Make it fast**: Performance improvements, advanced features

## 🎪 **Exception Handling**

**The ONLY time to deviate from "Working First":**
- When continuing would break existing working functionality
- When security vulnerabilities are introduced
- When the "simple" path creates data corruption risks

**Even then:** Fix properly, don't comment out or disable.
