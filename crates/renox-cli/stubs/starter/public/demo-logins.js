// The login page's seeded accounts (resources/views/renox/auth/login_options.html,
// shown in local only): a tap fills the form's email and password and moves to
// its button. A file rather than an inline script, so it runs under CSP=strict.
(function () {
  document.addEventListener("click", function (event) {
    var account = event.target.closest && event.target.closest("[data-demo-email]");
    if (!account) return;
    var email = document.querySelector("form input[name=email]");
    var form = email && email.form;
    if (!form || !form.elements.password) return;
    form.elements.email.value = account.getAttribute("data-demo-email");
    form.elements.password.value = account.getAttribute("data-demo-password");
    var submit = form.querySelector("[type=submit]");
    if (submit) submit.focus();
  });
})();
