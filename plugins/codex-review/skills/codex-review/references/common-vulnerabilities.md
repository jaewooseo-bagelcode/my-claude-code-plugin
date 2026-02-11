# Common Security Vulnerabilities Reference

Use this as a checklist when reviewing code for security issues.

## Injection Attacks

### SQL Injection
```javascript
// ❌ Vulnerable
const query = `SELECT * FROM users WHERE id = ${userId}`;

// ✅ Safe
const query = 'SELECT * FROM users WHERE id = ?';
db.execute(query, [userId]);
```

### Command Injection
```javascript
// ❌ Vulnerable
exec(`git log ${userInput}`);

// ✅ Safe
execFile('git', ['log', userInput]);
```

### NoSQL Injection
```javascript
// ❌ Vulnerable
db.find({ user: req.body.user });

// ✅ Safe
db.find({ user: sanitize(req.body.user) });
```

## XSS (Cross-Site Scripting)

### DOM-based XSS
```javascript
// ❌ Vulnerable
element.innerHTML = userInput;

// ✅ Safe
element.textContent = userInput;
```

### Reflected XSS
```javascript
// ❌ Vulnerable
res.send(`<h1>Hello ${req.query.name}</h1>`);

// ✅ Safe
res.send(`<h1>Hello ${escapeHtml(req.query.name)}</h1>`);
```

## Authentication & Authorization

### Weak Password Storage
```python
# ❌ Vulnerable
password_hash = md5(password)

# ✅ Safe
password_hash = bcrypt.hashpw(password, bcrypt.gensalt(rounds=12))
```

### JWT Vulnerabilities
```javascript
// ❌ Vulnerable
jwt.verify(token, secret, { algorithms: ['HS256', 'none'] });

// ✅ Safe
jwt.verify(token, secret, { algorithms: ['HS256'] });
```

### Session Fixation
```javascript
// ❌ Vulnerable
session.id = req.query.sessionId;

// ✅ Safe
session.regenerate();
```

## Path Traversal

```javascript
// ❌ Vulnerable
fs.readFile(`./uploads/${req.query.filename}`);

// ✅ Safe
const safePath = path.join('./uploads', path.basename(req.query.filename));
if (!safePath.startsWith(path.resolve('./uploads'))) throw new Error('Invalid path');
```

## Sensitive Data Exposure

### Hardcoded Secrets
```python
# ❌ Vulnerable
API_KEY = "sk-1234567890abcdef"

# ✅ Safe
API_KEY = os.environ.get("API_KEY")
```

### Logging Sensitive Data
```javascript
// ❌ Vulnerable
console.log('User login:', { email, password });

// ✅ Safe
console.log('User login:', { email });
```

## CSRF (Cross-Site Request Forgery)

```javascript
// ❌ Vulnerable (no CSRF protection)
app.post('/transfer', (req, res) => {
  transferMoney(req.body.amount);
});

// ✅ Safe
app.use(csrf());
app.post('/transfer', csrfProtection, (req, res) => {
  transferMoney(req.body.amount);
});
```

## Insecure Deserialization

```python
# ❌ Vulnerable
data = pickle.loads(user_input)

# ✅ Safe
data = json.loads(user_input)
```

## Use this reference when:
- Reviewing authentication/authorization code
- Checking user input handling
- Analyzing API endpoints
- Reviewing database queries
- Checking file operations
