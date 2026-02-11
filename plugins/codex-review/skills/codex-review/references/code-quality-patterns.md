# Code Quality Patterns Reference

Common anti-patterns and their fixes. Use when reviewing code quality.

## Function Complexity

### Long Functions (SRP Violation)
```javascript
// ❌ Bad: 100+ line function doing multiple things
function processUser(user) {
  // validate
  // transform
  // save to db
  // send email
  // log
  // update cache
}

// ✅ Good: Single responsibility
function processUser(user) {
  const validated = validateUser(user);
  const transformed = transformUser(validated);
  saveUser(transformed);
  notifyUser(transformed);
}
```

### Deep Nesting
```javascript
// ❌ Bad: Deep nesting
function process(data) {
  if (data) {
    if (data.items) {
      for (let item of data.items) {
        if (item.valid) {
          // deeply nested logic
        }
      }
    }
  }
}

// ✅ Good: Early returns
function process(data) {
  if (!data?.items) return;

  for (let item of data.items) {
    if (!item.valid) continue;
    // flat logic
  }
}
```

## Naming Conventions

### Unclear Names
```python
# ❌ Bad
def f(x, y):
    return x + y

# ✅ Good
def calculate_total_price(base_price, tax_amount):
    return base_price + tax_amount
```

### Magic Numbers
```javascript
// ❌ Bad
if (user.age < 18) return false;
setTimeout(fn, 86400000);

// ✅ Good
const MINIMUM_AGE = 18;
const ONE_DAY_MS = 24 * 60 * 60 * 1000;

if (user.age < MINIMUM_AGE) return false;
setTimeout(fn, ONE_DAY_MS);
```

## DRY (Don't Repeat Yourself)

```javascript
// ❌ Bad: Duplicated logic
function getUserById(id) {
  const user = db.query('SELECT * FROM users WHERE id = ?', [id]);
  return user;
}

function getUserByEmail(email) {
  const user = db.query('SELECT * FROM users WHERE email = ?', [email]);
  return user;
}

// ✅ Good: Abstraction
function getUserBy(field, value) {
  return db.query(`SELECT * FROM users WHERE ${field} = ?`, [value]);
}
```

## Error Handling

### Silent Failures
```python
# ❌ Bad
try:
    result = risky_operation()
except:
    pass  # Silent failure

# ✅ Good
try:
    result = risky_operation()
except SpecificError as e:
    logger.error(f"Operation failed: {e}")
    raise
```

### Generic Exceptions
```javascript
// ❌ Bad
catch (error) {
  return { error: 'Something went wrong' };
}

// ✅ Good
catch (error) {
  if (error instanceof ValidationError) {
    return { error: 'Invalid input', details: error.fields };
  }
  if (error instanceof DatabaseError) {
    logger.error('DB error:', error);
    return { error: 'Database unavailable' };
  }
  throw error; // Unknown errors should propagate
}
```

## SOLID Principles

### Single Responsibility Principle
```python
# ❌ Bad: Multiple responsibilities
class User:
    def save_to_database(self): ...
    def send_email(self): ...
    def generate_report(self): ...

# ✅ Good: Single responsibility
class User:
    pass

class UserRepository:
    def save(self, user): ...

class EmailService:
    def send_to_user(self, user): ...

class ReportGenerator:
    def generate_for_user(self, user): ...
```

## Use this reference when:
- Reviewing code readability
- Checking function complexity
- Analyzing code organization
- Evaluating maintainability
- Assessing code quality metrics
