import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { Router, provideRouter } from '@angular/router';
import { ConnectorsPage } from './connectors';

const connection = { provider: 'gitlab', external_account_id: '42', external_username: 'alice' };

describe('Connectors page', () => {
  beforeEach(() => {
    sessionStorage.setItem('authToken', 'local-session');
    TestBed.configureTestingModule({
      imports: [ConnectorsPage],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    });
  });
  afterEach(() => {
    TestBed.inject(HttpTestingController).verify();
    sessionStorage.clear();
    localStorage.clear();
  });

  function create() {
    const fixture = TestBed.createComponent(ConnectorsPage);
    fixture.detectChanges();
    return fixture;
  }

  function enterToken(fixture: ReturnType<typeof create>, token = 'glpat-example') {
    const input = fixture.nativeElement.querySelector('#gitlab-token');
    input.value = token;
    input.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    fixture.nativeElement.querySelector('form').dispatchEvent(new Event('submit', { cancelable: true }));
    fixture.detectChanges();
  }

  it('keeps both provider tokens masked without a reveal control', () => {
    const fixture = create();
    expect(fixture.nativeElement.querySelector('#gitlab-token').type).toBe('password');
    expect(fixture.nativeElement.querySelector('button[type="submit"]').disabled).toBe(true);
    expect(fixture.nativeElement.querySelector('#github-token').disabled).toBe(false);
    expect(fixture.nativeElement.querySelector('#github-token').type).toBe('password');
    TestBed.inject(HttpTestingController).expectNone('/connectors/github/authorize');
    expect(fixture.nativeElement.querySelector('.visibility-toggle')).toBeNull();
  });

  it('saves using the existing endpoint and clears the token after success without storing it in the browser', async () => {
    const fixture = create();
    enterToken(fixture, '  glpat-example  ');
    const request = TestBed.inject(HttpTestingController).expectOne('/connectors/gitlab/authorize');
    expect(request.request.method).toBe('POST');
    expect(request.request.headers.get('Authorization')).toBe('Bearer local-session');
    expect(request.request.body).toEqual({ token: 'glpat-example' });
    expect(fixture.nativeElement.querySelector('#gitlab-token').disabled).toBe(true);
    expect(fixture.nativeElement.querySelector('button[type="submit"]').textContent).toContain('Saving');
    fixture.nativeElement.querySelector('form').dispatchEvent(new Event('submit', { cancelable: true }));
    TestBed.inject(HttpTestingController).expectNone('/connectors/gitlab/authorize');
    request.flush(connection);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('#gitlab-token').value).toBe('');
    expect(fixture.nativeElement.querySelector('#gitlab-token').type).toBe('password');
    expect(fixture.nativeElement.textContent).not.toContain('glpat-example');
    expect(fixture.nativeElement.querySelector('[role="status"]').textContent).toContain('Connected as alice');
    expect(JSON.stringify(sessionStorage)).not.toContain('glpat-example');
    expect(JSON.stringify(localStorage)).not.toContain('glpat-example');
  });

  it('saves GitHub independently of GitLab and clears the token after success without storing it in the browser', async () => {
    const fixture = create();
    const input = fixture.nativeElement.querySelector('#github-token');
    input.value = '  github_pat_example  ';
    input.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    fixture.nativeElement.querySelector('[aria-labelledby="github-heading"] form').dispatchEvent(new Event('submit', { cancelable: true }));
    fixture.detectChanges();
    const request = TestBed.inject(HttpTestingController).expectOne('/connectors/github/authorize');
    expect(request.request.method).toBe('POST');
    expect(request.request.headers.get('Authorization')).toBe('Bearer local-session');
    expect(request.request.body).toEqual({ token: 'github_pat_example' });
    expect(fixture.nativeElement.querySelector('#github-token').disabled).toBe(true);
    expect(fixture.nativeElement.querySelector('[aria-labelledby="github-heading"] button').textContent).toContain('Saving');
    fixture.nativeElement.querySelector('[aria-labelledby="github-heading"] form').dispatchEvent(new Event('submit', { cancelable: true }));
    TestBed.inject(HttpTestingController).expectNone('/connectors/github/authorize');
    request.flush({ ...connection, provider: 'github' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('#github-token').value).toBe('');
    expect(fixture.nativeElement.querySelector('#github-token').type).toBe('password');
    expect(fixture.nativeElement.textContent).not.toContain('github_pat_example');
    expect(fixture.nativeElement.querySelector('[role="status"]').textContent).toContain('Connected as alice');
    expect(JSON.stringify(sessionStorage)).not.toContain('github_pat_example');
    expect(JSON.stringify(localStorage)).not.toContain('github_pat_example');
  });

  it('explains rejected read permissions and allows retry', async () => {
    const fixture = create();
    enterToken(fixture);
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/connectors/gitlab/authorize').flush('Missing permissions', { status: 400, statusText: 'Bad request' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('[role="alert"]').textContent).toContain('read_api and read_repository');
    expect(fixture.nativeElement.querySelector('#gitlab-token').value).toBe('glpat-example');
    fixture.nativeElement.querySelector('form').dispatchEvent(new Event('submit', { cancelable: true }));
    http.expectOne('/connectors/gitlab/authorize').flush(connection);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('[role="alert"]')).toBeNull();
  });

  it('reports unavailable configuration without showing a saved connection', async () => {
    const fixture = create();
    enterToken(fixture);
    TestBed.inject(HttpTestingController).expectOne('/connectors/gitlab/authorize').flush('Not configured', { status: 503, statusText: 'Unavailable' });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('GitLab connections are unavailable');
    expect(fixture.nativeElement.querySelector('[role="status"]')).toBeNull();
  });

  it('clears the token and returns to login when the local session expires', async () => {
    const fixture = create();
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    enterToken(fixture);
    TestBed.inject(HttpTestingController).expectOne('/connectors/gitlab/authorize').flush('Expired', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(navigate).toHaveBeenCalledWith('/login');
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(fixture.nativeElement.querySelector('#gitlab-token').value).toBe('');
  });

  it('rejects malformed tokens before requesting and cancels pending requests when leaving', () => {
    const fixture = create();
    enterToken(fixture, 'token with spaces');
    const http = TestBed.inject(HttpTestingController);
    http.expectNone('/connectors/gitlab/authorize');
    expect(fixture.nativeElement.querySelector('[role="alert"]').textContent).toContain('without spaces');
    enterToken(fixture);
    const request = http.expectOne('/connectors/gitlab/authorize');
    fixture.destroy();
    expect(request.cancelled).toBe(true);
  });
});
