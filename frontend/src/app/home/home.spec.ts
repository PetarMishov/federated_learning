import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { Home } from './home';

const organizations = [
  { id: 1, name: 'Central Hospital', owner_user_id: 1 },
  { id: 2, name: 'Research Lab', owner_user_id: 1 },
];

describe('Home organizations', () => {
  beforeEach(() => {
    sessionStorage.clear();
    TestBed.configureTestingModule({
      imports: [Home],
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });
  });

  afterEach(() => {
    TestBed.inject(HttpTestingController).verify();
    sessionStorage.clear();
  });

  it('asks for sign-in without requesting private data when there is no token', async () => {
    const fixture = TestBed.createComponent(Home);
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Sign in to see your organizations.');
    TestBed.inject(HttpTestingController).expectNone('/users/organizations');
  });

  it('signs in, sends the returned bearer token, and renders organizations', async () => {
    const fixture = TestBed.createComponent(Home);
    await fixture.whenStable();
    const page: HTMLElement = fixture.nativeElement;
    const username = page.querySelector<HTMLInputElement>('#username')!;
    const password = page.querySelector<HTMLInputElement>('#password')!;
    username.value = 'demo';
    username.dispatchEvent(new Event('input'));
    password.value = 'demo-password';
    password.dispatchEvent(new Event('input'));
    await fixture.whenStable();
    page.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
    const http = TestBed.inject(HttpTestingController);
    const login = http.expectOne('/users/verify');
    expect(login.request.method).toBe('POST');
    expect(login.request.body).toEqual({ username: 'demo', password: 'demo-password' });
    login.flush('returned-token');
    const request = http.expectOne('/users/organizations');
    expect(request.request.method).toBe('GET');
    expect(request.request.headers.get('Authorization')).toBe('Bearer returned-token');
    expect(request.request.params.keys()).toEqual([]);
    request.flush({ organizations });
    await fixture.whenStable();
    expect(page.querySelector('h1')?.textContent).toBe('Welcome, demo!');
    expect(page.querySelectorAll('.organization-list li').length).toBe(2);
    expect(page.textContent).toContain('Central Hospital');
    expect(page.querySelector('#password')).toBeNull();
  });

  it('loads with an existing session and handles an empty membership list', async () => {
    sessionStorage.setItem('authToken', 'saved-token');
    const fixture = TestBed.createComponent(Home);
    fixture.detectChanges();
    const request = TestBed.inject(HttpTestingController).expectOne('/users/organizations');
    expect(request.request.headers.get('Authorization')).toBe('Bearer saved-token');
    request.flush({ organizations: [] });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('You are not a member of any organizations yet.');
  });

  it('clears an expired session and asks for sign-in again', async () => {
    sessionStorage.setItem('authToken', 'expired-token');
    const fixture = TestBed.createComponent(Home);
    fixture.detectChanges();
    TestBed.inject(HttpTestingController).expectOne('/users/organizations')
      .flush('Unauthorized', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(fixture.nativeElement.querySelector('form')).toBeTruthy();
    expect(fixture.nativeElement.querySelector('[role="alert"]').textContent).toContain('Please sign in');
  });

  it('offers a retry after a server error', async () => {
    sessionStorage.setItem('authToken', 'saved-token');
    const fixture = TestBed.createComponent(Home);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/users/organizations').flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    fixture.nativeElement.querySelector('button').click();
    http.expectOne('/users/organizations').flush({ organizations });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Central Hospital');
  });
});
