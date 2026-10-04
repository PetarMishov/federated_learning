import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { App } from './app';
import { routes } from './app.routes';

describe('Authentication routes', () => {
  beforeEach(async () => {
    sessionStorage.clear();
    await TestBed.configureTestingModule({
      imports: [App],
      providers: [provideRouter(routes), provideHttpClient(), provideHttpClientTesting()],
    }).compileComponents();
  });

  afterEach(() => {
    TestBed.inject(HttpTestingController).verify();
    sessionStorage.clear();
  });

  it('starts on login without a sidebar and guards home', async () => {
    const fixture = TestBed.createComponent(App);
    const router = TestBed.inject(Router);
    await router.navigateByUrl('/');
    await fixture.whenStable();
    expect(router.url).toBe('/login');
    expect(fixture.nativeElement.querySelector('aside')).toBeNull();
    expect(fixture.nativeElement.querySelector('#username')).toBeTruthy();
    expect(fixture.nativeElement.querySelector('#password')).toBeTruthy();
    await router.navigateByUrl('/home');
    expect(router.url).toBe('/login');
    TestBed.inject(HttpTestingController).expectNone('/users/organizations');
    TestBed.inject(HttpTestingController).expectNone('/users/notifications');
  });

  it('logs in, loads private organizations, and revokes the token on logout', async () => {
    const fixture = TestBed.createComponent(App);
    const router = TestBed.inject(Router);
    const http = TestBed.inject(HttpTestingController);
    await router.navigateByUrl('/login');
    await fixture.whenStable();
    const page: HTMLElement = fixture.nativeElement;
    for (const [selector, value] of [['#username', 'demo'], ['#password', 'demo-password']]) {
      const input = page.querySelector<HTMLInputElement>(selector)!;
      input.value = value;
      input.dispatchEvent(new Event('input'));
    }
    await fixture.whenStable();
    page.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
    const login = http.expectOne('/users/verify');
    expect(login.request.body).toEqual({ username: 'demo', password: 'demo-password' });
    login.flush('signed-token');
    await fixture.whenStable();
    expect(router.url).toBe('/home');
    const organizations = http.expectOne('/users/organizations');
    expect(organizations.request.headers.get('Authorization')).toBe('Bearer signed-token');
    organizations.flush({ organizations: [{ id: 1, name: 'Central Hospital', owner_user_id: 1 }] });
    const notifications = http.expectOne('/users/notifications');
    expect(notifications.request.headers.get('Authorization')).toBe('Bearer signed-token');
    notifications.flush({ notifications: [{ id: 1, title: 'Welcome notification', is_read: false, created_at: Date.UTC(2026, 9, 4, 12, 30) }] });
    await fixture.whenStable();
    expect(page.textContent).toContain('Welcome, demo!');
    expect(page.textContent).toContain('Central Hospital');
    expect(page.textContent).toContain('Welcome notification');
    expect(page.querySelector('aside')).toBeTruthy();
    page.querySelector<HTMLButtonElement>('.logout')!.click();
    const logout = http.expectOne('/users/logout');
    expect(logout.request.method).toBe('POST');
    expect(logout.request.headers.get('Authorization')).toBe('Bearer signed-token');
    logout.flush(null, { status: 204, statusText: 'No Content' });
    await fixture.whenStable();
    expect(router.url).toBe('/login');
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(page.querySelector('aside')).toBeNull();
    await router.navigateByUrl('/home');
    expect(router.url).toBe('/login');
  });

  it('keeps login visible when credentials are rejected', async () => {
    const fixture = TestBed.createComponent(App);
    await TestBed.inject(Router).navigateByUrl('/login');
    await fixture.whenStable();
    for (const selector of ['#username', '#password']) {
      const input = fixture.nativeElement.querySelector(selector);
      input.value = 'wrong';
      input.dispatchEvent(new Event('input'));
    }
    await fixture.whenStable();
    fixture.nativeElement.querySelector('form').dispatchEvent(new Event('submit'));
    TestBed.inject(HttpTestingController).expectOne('/users/verify').flush('', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Invalid username or password.');
    expect(fixture.nativeElement.querySelector('aside')).toBeNull();
    expect(sessionStorage.getItem('authToken')).toBeNull();
  });
});
